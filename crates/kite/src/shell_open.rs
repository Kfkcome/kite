//! Opening, switching and closing tabs: a host becomes a tab with one or more panes, each its
//! own SSH session.
//!
//! Splits: a tab holds one or more panes laid out as a tree (see `split.rs`). A split opens
//! another session like the focused one; ⌘W closes a pane before it closes the tab.

use crate::split::{Axis, Direction, Node, PaneId};
use crate::tabs::DraggedTab;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, DragMoveEvent, Focusable, InteractiveElement, IntoElement, ParentElement,
    Styled, Window, div, px,
};
use kite_ssh::{ConnectSpec, HostEntry};

use super::*;
use crate::i18n::tr;

impl Shell {
    /// Focuses the host's existing pane when one is open — the first, in tab order, then
    /// pane order — and reconnects it when dormant. Otherwise opens a new tab.
    pub fn connect_host(&mut self, host: &HostEntry, window: &mut Window, cx: &mut Context<Self>) {
        self.record_recent(&host.alias);
        if let Some((ix, pane_id)) = self.locate_host(&host.alias) {
            let session = self.tabs.get(ix).and_then(|tab| {
                tab.panes
                    .iter()
                    .find(|pane| pane.id == pane_id)
                    .map(|pane| pane.session.clone())
            });
            if let Some(session) = session {
                session.update(cx, |session, cx| {
                    if session.status.is_dormant() {
                        session.reconnect(cx);
                    }
                });
            }
            return self.show_pane(ix, pane_id, window, cx);
        }
        match ConnectSpec::from_host_entry(host) {
            Ok(spec) => self.open_tab(Launch::Ssh(spec), &host.alias, window, cx),
            Err(e) => self.fail(e.to_string(), cx),
        }
    }

    /// `tern <target>`: a `~/.ssh/config` alias or `user@host:port`.
    pub fn connect_target(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        if crate::layouts::is_local_alias(target) {
            return self.open_local_terminal(window, cx);
        }
        // A saved connection's name wins, so `tern web-01` and the picker's typed name open the
        // same host (with its password command, jump host and group) as clicking it does.
        if let Some(host) = self.hosts.iter().find(|h| h.alias == target).cloned() {
            return self.connect_host(&host, window, cx);
        }
        match ConnectSpec::parse(target) {
            Ok(spec) => self.open_tab(Launch::Ssh(spec), target, window, cx),
            Err(e) => self.fail(e.to_string(), cx),
        }
    }

    /// Opens a new tab running the user's local login shell.
    pub fn open_local_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = tr("Local Terminal").to_owned();
        self.open_tab(Launch::Local, &name, window, cx);
    }

    pub(super) fn open_tab(
        &mut self,
        launch: Launch,
        alias: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.make_pane(launch, alias, window, cx);
        self.next_tab += 1;
        self.tabs.push(Tab {
            id: self.next_tab,
            title: None,
            layout_id: None,
            tree: split::Node::Leaf(pane.id),
            focused: pane.id,
            panes: vec![pane],
        });
        self.error = None;
        self.show_tab(self.tabs.len() - 1, window, cx);
    }

    /// A session with the settings applied, and an eye on it for the day it drops.
    pub(super) fn make_pane(
        &mut self,
        launch: Launch,
        alias: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Pane {
        // Debug builds: keep scripted logins out of the real known_hosts file.
        #[cfg(debug_assertions)]
        let launch = match launch {
            Launch::Ssh(spec) => Launch::Ssh(with_dev_known_hosts(spec)),
            Launch::SshIdle(spec) => Launch::SshIdle(with_dev_known_hosts(spec)),
            Launch::Local | Launch::LocalIn(_) => launch,
        };
        let theme = self.terminal_theme(alias);
        let auth = self.auth_for(alias);
        let session = Session::open(
            launch,
            auth,
            alias.to_owned(),
            self.settings.log_sessions,
            theme,
            window,
            cx,
        );
        let meta = self.settings.option_as_meta;
        let view = session.read(cx).view.clone();
        let options = self.terminal_options();
        view.update(cx, |v, cx| {
            v.set_option_as_meta(meta);
            v.set_options(options, cx);
        });
        let cwd = session
            .read(cx)
            .local_cwd()
            .map(std::path::Path::to_path_buf);
        let bell = self.on_bell(&session, window, cx);
        // A tab that is not in front can drop without anyone seeing its terminal; say so, with
        // a way to get to it.
        let mut last = session.read(cx).status.clone();
        let repaint = cx.observe(&session, move |shell, session, cx| {
            let status = session.read(cx).status.clone();
            if status == Status::Closed && last == Status::Connected {
                let hit = shell.tabs.iter().enumerate().find_map(|(ix, tab)| {
                    tab.panes.iter().find_map(|pane| {
                        (pane.session == session).then(|| (ix, pane.alias.clone()))
                    })
                });
                if let Some((_, alias)) =
                    hit.filter(|(ix, _)| *ix != shell.active || shell.settings_page.is_some())
                {
                    shell.disconnect_toast(&alias, cx);
                }
            } else if status == Status::Connected && last != Status::Connected {
                shell.reconcile_disconnect_toasts(cx);
            }
            last = status;
            cx.notify();
        });
        let notes = cx.subscribe(
            &session,
            |shell, session, note: &crate::session::SessionNote, cx| {
                shell.on_session_note(&session, note, cx)
            },
        );
        self.next_pane += 1;
        Pane {
            id: self.next_pane,
            alias: alias.to_owned(),
            cwd,
            session,
            _repaint: repaint,
            _bell: bell,
            _notes: notes,
        }
    }

    /// Makes tab `ix` the active one. Settings, when open, stays open over it: this runs for
    /// reorders, closes and restores too, which are not a request to see the session.
    pub fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        self.active = ix;
        self.persist_tabs();
        self.tab_scroll.scroll_to_item(ix);
        // Behind Settings the terminal is hidden, and keys sent to it would be typed blind.
        if self.settings_page.is_none() {
            let focus = tab.session().read(cx).view.focus_handle(cx);
            window.focus(&focus, cx);
        }
        // After `tab` is released: this needs `&mut self` and only queues a background read.
        self.refresh_local_cwds(cx);
        self.apply_broadcast(cx);
        self.reconcile_disconnect_toasts(cx);
        cx.notify();
    }

    /// Shows tab `ix` because the user asked for that session, so Settings closes.
    pub fn show_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_page = None;
        self.activate_tab(ix, window, cx);
    }

    /// Focuses `pane` when it is still in that tab's tree, then shows the tab.
    pub(super) fn show_pane(
        &mut self,
        ix: usize,
        pane: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.tabs.get_mut(ix)
            && tab.tree.contains(pane)
        {
            tab.focused = pane;
        }
        self.show_tab(ix, window, cx);
    }

    /// Closing a tab drops its session, which ends the connection.
    pub fn close_tab_at(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.renaming = None;
        let dying: Vec<_> = self.tabs[ix]
            .panes
            .iter()
            .map(|pane| pane.session.clone())
            .collect();
        self.migrate_forwards(&dying, cx);
        self.tabs.remove(ix);
        self.sync_broadcast(cx);
        if self.tabs.is_empty() {
            self.active = 0;
            self.close_orphan_panels(window, cx);
            self.persist_tabs();
            self.reconcile_disconnect_toasts(cx);
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        let next = if self.active > ix || self.active == self.tabs.len() {
            self.active - 1
        } else {
            self.active
        };
        self.activate_tab(next, window, cx);
        self.close_orphan_panels(window, cx);
    }
}

/// 1320×880; debug builds take `KITE_DEV_WINDOW=900x600` so a scripted check can open the
/// window at its minimum.
fn start_size() -> (f32, f32) {
    #[cfg(debug_assertions)]
    if let Some((w, h)) = std::env::var("KITE_DEV_WINDOW")
        .ok()
        .and_then(|v| v.split_once('x').map(|(w, h)| (w.parse(), h.parse())))
        .and_then(|(w, h)| Some((w.ok()?, h.ok()?)))
    {
        return (w, h);
    }
    (1320., 880.)
}

/// Centred on the main display; debug builds take `KITE_DEV_WINDOW_AT=x,y` to place it, e.g. on
/// a 1× external display.
pub(super) fn start_bounds(cx: &gpui::App) -> gpui::Bounds<gpui::Pixels> {
    let (w, h) = start_size();
    #[allow(unused_mut)]
    let mut bounds = gpui::Bounds::centered(None, gpui::size(gpui::px(w), gpui::px(h)), cx);
    #[cfg(debug_assertions)]
    if let Some((x, y)) = std::env::var("KITE_DEV_WINDOW_AT")
        .ok()
        .and_then(|v| v.split_once(',').map(|(x, y)| (x.parse(), y.parse())))
        .and_then(|(x, y)| Some((x.ok()?, y.ok()?)))
    {
        bounds.origin = gpui::point(gpui::px(x), gpui::px(y));
    }
    bounds
}

impl Shell {
    /// Splits the focused pane of the front tab; the new pane has the keyboard.
    pub(crate) fn split_pane(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let launch = tab.session().read(cx).launch_again();
        let alias = tab.focused_alias().to_owned();
        let (target, ix) = (tab.focused, self.active);
        let pane = self.make_pane(launch, &alias, window, cx);
        let id = pane.id;
        let Some(tab) = self.tabs.get_mut(ix) else {
            return;
        };
        if !tab.tree.split(target, axis, id) {
            return;
        }
        tab.panes.push(pane);
        tab.focused = id;
        self.focus_front_pane(window, cx);
        self.persist_tabs();
    }

    /// ⌘W: a split tab loses the focused pane, a tab with one pane closes.
    pub(crate) fn close_pane_or_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let (tab_id, focused) = (tab.id, tab.focused);
        self.close_pane_in_tab(tab_id, focused, window, cx);
    }

    /// Closes `pane_id` inside `tab_id` (ending its session); if it was the tab's only pane,
    /// closes the tab instead. Keeps focus on the active pane when another pane is closed.
    pub(crate) fn close_pane_in_tab(
        &mut self,
        tab_id: broadcast_ui::TabId,
        pane_id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab_ix) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        let (dying, is_only, next) = {
            let tab = &self.tabs[tab_ix];
            let Some(dying) = tab
                .panes
                .iter()
                .find(|pane| pane.id == pane_id)
                .map(|pane| pane.session.clone())
            else {
                return;
            };
            if tab.panes.len() <= 1 {
                (dying, true, None)
            } else {
                let order = tab.tree.leaves();
                let Some(next) = next_focus_after_close(&order, tab.focused, pane_id) else {
                    return;
                };
                if tab.tree.clone().remove(pane_id).is_none() {
                    return;
                }
                (dying, false, Some(next))
            }
        };
        if is_only {
            self.close_tab_at(tab_ix, window, cx);
            return;
        }
        let Some(next_focus) = next else {
            return;
        };
        self.migrate_forwards(std::slice::from_ref(&dying), cx);
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let tree = std::mem::replace(&mut tab.tree, Node::Leaf(pane_id));
        let Some(rest) = tree.remove(pane_id) else {
            return;
        };
        tab.tree = rest;
        tab.panes.retain(|pane| pane.id != pane_id);
        tab.focused = next_focus;
        if self.tabs.get(self.active).is_some_and(|t| t.id == tab_id) {
            self.focus_front_pane(window, cx);
        } else {
            self.apply_broadcast(cx);
        }
        self.reconcile_disconnect_toasts(cx);
        self.close_orphan_panels(window, cx);
        self.persist_tabs();
    }

    pub(crate) fn focus_pane(&mut self, id: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        if tab.focused == id || !tab.tree.contains(id) {
            return;
        }
        tab.focused = id;
        self.focus_front_pane(window, cx);
        self.persist_tabs();
    }

    pub(crate) fn move_pane_focus(
        &mut self,
        dir: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if let Some(to) = tab.tree.neighbour(tab.focused, dir) {
            self.focus_pane(to, window, cx);
        }
    }

    /// Gives the keyboard to the focused pane of the front tab, and points a broadcast at it.
    pub(super) fn focus_front_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            let focus = tab.session().read(cx).view.focus_handle(cx);
            window.focus(&focus, cx);
        }
        self.apply_broadcast(cx);
        cx.notify();
    }

    /// The tab's terminals, in their split layout.
    pub(super) fn render_panes(&self, tab: &Tab, cx: &mut Context<Self>) -> AnyElement {
        self.render_node(tab, &tab.tree, tab.panes.len() > 1, cx)
    }

    pub(super) fn render_node(
        &self,
        tab: &Tab,
        node: &Node,
        split: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            Node::Leaf(id) => {
                let Some(pane) = tab.panes.iter().find(|p| p.id == *id) else {
                    return div().into_any_element();
                };
                let id = *id;
                let tab_id = tab.id;
                let focused = tab.focused == id;
                let alias = pane.alias.clone();
                let (label, view) = {
                    let session = pane.session.read(cx);
                    (session.label(), session.view.clone())
                };
                div()
                    .id(("pane", id))
                    .relative()
                    .size_full()
                    // A split stacks a header over the terminal. No frame: a box around
                    // every pane stacks with the seam and reads as a thick border.
                    .when(split, |el| el.flex().flex_col())
                    .when(split, |el| {
                        el.child(self.pane_title_bar(tab_id, id, &alias, &label, focused, cx))
                    })
                    .child(if split {
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .min_w(px(0.))
                            // Before the terminal sees the click, so selecting text in a pane also
                            // gives it the keyboard without catching clicks on the close button.
                            .capture_any_mouse_down(
                                cx.listener(move |s, _, w, cx| s.focus_pane(id, w, cx)),
                            )
                            .child(view)
                            .into_any_element()
                    } else {
                        view.into_any_element()
                    })
                    .children(self.split_drop_overlay(tab, id, cx))
                    .on_drag_move(cx.listener(
                        move |shell, event: &DragMoveEvent<DraggedTab>, _, cx| {
                            shell.on_tab_drag_over_pane(event, tab_id, id, cx);
                        },
                    ))
                    .on_drop(cx.listener(move |shell, from: &DraggedTab, window, cx| {
                        shell.accept_tab_drop(from, tab_id, id, window, cx);
                    }))
                    .on_drag_move(cx.listener(
                        move |shell, event: &DragMoveEvent<DraggedPane>, _, cx| {
                            shell.on_pane_drag_over_pane(event, tab_id, id, cx);
                        },
                    ))
                    .on_drop(cx.listener(move |shell, from: &DraggedPane, window, cx| {
                        shell.accept_pane_rearrange(from, tab_id, id, window, cx);
                    }))
                    .into_any_element()
            }
            Node::Split {
                axis,
                ratio,
                first,
                second,
            } => self.render_split(tab, *axis, *ratio, first, second, cx),
        }
    }
}

/// Which pane holds focus after `closing` is removed from a multi-pane tab.
/// Keeps `focused` when closing a different pane; otherwise moves to the previous leaf
/// (or the second leaf when closing the first). `None` if `closing` is missing or the only leaf.
fn next_focus_after_close(leaves: &[PaneId], focused: PaneId, closing: PaneId) -> Option<PaneId> {
    if leaves.len() < 2 {
        return None;
    }
    let at = leaves.iter().position(|&id| id == closing)?;
    if focused != closing && leaves.contains(&focused) {
        Some(focused)
    } else if at > 0 {
        Some(leaves[at - 1])
    } else {
        Some(leaves[1])
    }
}

#[cfg(test)]
mod tests {
    use super::next_focus_after_close;

    #[test]
    fn closing_an_unfocused_pane_keeps_the_focused_one() {
        assert_eq!(next_focus_after_close(&[1, 2, 3], 2, 1), Some(2));
        assert_eq!(next_focus_after_close(&[1, 2, 3], 1, 2), Some(1));
        assert_eq!(next_focus_after_close(&[1, 2, 3], 2, 3), Some(2));
    }

    #[test]
    fn closing_the_focused_pane_moves_to_neighbour_leaf() {
        assert_eq!(next_focus_after_close(&[1, 2, 3], 1, 1), Some(2));
        assert_eq!(next_focus_after_close(&[1, 2, 3], 2, 2), Some(1));
        assert_eq!(next_focus_after_close(&[1, 2, 3], 3, 3), Some(2));
    }

    #[test]
    fn closing_single_or_missing_pane_returns_none_and_stale_focus_falls_back() {
        assert_eq!(next_focus_after_close(&[1], 1, 1), None);
        assert_eq!(next_focus_after_close(&[], 1, 1), None);
        assert_eq!(next_focus_after_close(&[1, 2], 1, 9), None);
        assert_eq!(next_focus_after_close(&[1, 2, 3], 99, 2), Some(1));
        assert_eq!(next_focus_after_close(&[1, 2, 3], 99, 1), Some(2));
    }
}
