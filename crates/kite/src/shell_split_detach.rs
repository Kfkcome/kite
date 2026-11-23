//! A split pane's title bar, and dragging that bar: onto the strip to detach the pane into its
//! own tab, or onto a sibling pane of the same tab to rearrange it.

use gpui::{
    AnyElement, App, AppContext, Context, DragMoveEvent, Hsla, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::split::{Node, PaneId, Side};
use crate::theme::CONTROL_RADIUS;

use super::broadcast_ui::TabId;
use super::{Pane, Shell, Tab};

/// A pane being dragged by its title bar, out of a split tab.
#[derive(Clone)]
pub struct DraggedPane {
    /// The tab this pane belongs to.
    pub tab: TabId,
    /// The pane being dragged.
    pub pane: PaneId,
    alias: SharedString,
    background: Hsla,
    color: Hsla,
}

impl Render for DraggedPane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(18.))
            .px(px(8.))
            .flex()
            .items_center()
            .rounded(px(CONTROL_RADIUS))
            .bg(self.background)
            .text_color(self.color)
            .text_size(px(11.))
            .shadow_md()
            .child(self.alias.clone())
    }
}

impl Shell {
    /// The host line above one pane of a split. A lone pane draws none.
    /// `label` is `user@host:port`; the drag ghost keeps the short alias.
    pub(super) fn pane_title_bar(
        &self,
        tab: TabId,
        pane: PaneId,
        alias: &str,
        label: &str,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::hover::{Accessible as _, HoverFade as _};
        let t = self.theme;
        let term = self.terminal_theme(alias);
        let dragged = DraggedPane {
            tab,
            pane,
            alias: SharedString::from(alias.to_owned()),
            background: t.popup,
            color: t.text,
        };
        let pane_btn =
            |id_name: &'static str, tip: &'static str, glyph: &'static str, icon_size: f32| {
                let key = format!("{id_name}-{pane}");
                let fg = crate::hover::blend(&key, t.muted, t.text);
                div()
                    .id((id_name, pane))
                    .size(px(18.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover_fade(key, gpui::transparent_black(), t.row_hover)
                    .icon_button(crate::i18n::tr(tip), &t)
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(crate::icons::icon(glyph).size(px(icon_size)).text_color(fg))
            };
        let pane_session = move |shell: &Shell| -> Option<gpui::Entity<crate::session::Session>> {
            shell
                .tabs
                .iter()
                .find(|t| t.id == tab)
                .and_then(|t| t.panes.iter().find(|p| p.id == pane))
                .map(|p| p.session.clone())
        };
        div()
            .id(("pane-title-bar", pane))
            .h(px(22.))
            .pr(px(4.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .min_w_0()
            .bg(term.background.opacity(term.background_alpha))
            .text_size(px(12.))
            .text_color(if focused { t.text } else { t.muted })
            .child(
                div()
                    .id(("pane-title", pane))
                    .flex_1()
                    .h_full()
                    .pl(px(10.))
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .capture_any_mouse_down(cx.listener(move |shell, _, window, cx| {
                        shell.focus_pane(pane, window, cx);
                    }))
                    // The drag starts here, not on the terminal, so selecting text still works.
                    .on_drag(dragged, |pane, _, _, cx| cx.new(|_| pane.clone()))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(label.to_owned())),
                    )
                    .child(
                        pane_btn("pane-sftp", "Browse files", crate::icons::FOLDER, 12.).on_click(
                            cx.listener(move |shell, _, window, cx| {
                                if let Some(session) = pane_session(shell) {
                                    shell.open_sftp_for_session(session, window, cx);
                                }
                            }),
                        ),
                    )
                    .child(
                        pane_btn("pane-forwards", "Port forwards", crate::icons::GLOBE, 12.)
                            .on_click(cx.listener(
                                move |shell, e: &gpui::ClickEvent, window, cx| {
                                    if let Some(session) = pane_session(shell) {
                                        shell.open_forwards_for_session(
                                            session,
                                            e.position(),
                                            window,
                                            cx,
                                        );
                                    }
                                },
                            )),
                    )
                    .child(
                        pane_btn(
                            "pane-reconnect",
                            "Reconnect current session",
                            crate::icons::RESTART,
                            12.,
                        )
                        .on_click(cx.listener(
                            move |shell, _, window, cx| {
                                if let Some(session) = pane_session(shell) {
                                    session.update(cx, |s, cx| s.reconnect(cx));
                                    shell.focus_pane(pane, window, cx);
                                }
                            },
                        )),
                    ),
            )
            .child(
                pane_btn("pane-close", "Close pane", crate::icons::CLOSE, 14.).on_click(
                    cx.listener(move |shell, _, window, cx| {
                        cx.stop_propagation();
                        shell.close_pane_in_tab(tab, pane, window, cx);
                    }),
                ),
            )
            .into_any_element()
    }

    pub(super) fn clear_pane_insert_without_drag(&mut self, cx: &App) {
        if self.pane_insert.is_some() && !cx.has_active_drag() {
            self.pane_insert = None;
        }
    }

    /// The pointer is over pill `hover`. The slot follows [`pane_insert_slot`]; a sibling pill
    /// does not clear a hint it does not own.
    pub(crate) fn on_pane_drag_over_pill(
        &mut self,
        event: &DragMoveEvent<DraggedPane>,
        hover: usize,
        cx: &mut Context<Self>,
    ) {
        self.track_pane_insert(event, InsertOwner::Pill(hover), cx, |rel_x, len| {
            pane_insert_slot(hover, rel_x, len)
        });
    }

    /// The pointer is over the empty area after the last pill: insert at the end.
    pub(crate) fn on_pane_drag_over_strip_end(
        &mut self,
        event: &DragMoveEvent<DraggedPane>,
        cx: &mut Context<Self>,
    ) {
        self.track_pane_insert(event, InsertOwner::End, cx, |_, len| Some(len));
    }

    fn track_pane_insert(
        &mut self,
        event: &DragMoveEvent<DraggedPane>,
        owner: InsertOwner,
        cx: &mut Context<Self>,
        slot_at: impl FnOnce(f32, usize) -> Option<usize>,
    ) {
        let inside = super::split_drag::pane_rel(
            f32::from(event.event.position.x),
            f32::from(event.event.position.y),
            f32::from(event.bounds.origin.x),
            f32::from(event.bounds.origin.y),
            f32::from(event.bounds.size.width),
            f32::from(event.bounds.size.height),
        );
        if let Some((rel_x, _)) = inside
            && let Some(slot) = slot_at(rel_x, self.tabs.len())
        {
            let next = PaneInsertHint { owner, slot };
            if self.pane_insert != Some(next) {
                self.pane_insert = Some(next);
                cx.notify();
            }
            return;
        }
        if self.pane_insert.is_some_and(|hint| hint.owner == owner) {
            self.pane_insert = None;
            cx.notify();
        }
    }

    /// Pulls the dragged pane into a new tab at the stored slot. The session stays up.
    /// A failed check clears the hint and leaves every tab as it was.
    pub(crate) fn accept_pane_detach(
        &mut self,
        dragged: &DraggedPane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.pane_insert.take().map(|hint| hint.slot) else {
            cx.notify();
            return;
        };
        let new_id = self.next_tab + 1;
        let Some(active) = detach_pane(&mut self.tabs, dragged.tab, dragged.pane, slot, new_id)
        else {
            cx.notify();
            return;
        };
        self.next_tab = new_id;
        self.renaming = None;
        self.sync_broadcast(cx);
        self.close_orphan_panels(window, cx);
        self.show_tab(active, window, cx);
    }
}

/// Which strip target owns the live insertion slot. A sibling must not clear one it does not own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InsertOwner {
    Pill(usize),
    /// The empty area after the last pill.
    End,
}

/// Where a dragged pane would be inserted in the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PaneInsertHint {
    owner: InsertOwner,
    pub(super) slot: usize,
}

/// Slot at which to insert a new tab when the pointer is over pill `hover`
/// (0-based) in a strip of `len` tabs. `rel_x` is the pointer's x inside the
/// pill, in 0..=1. The left half (`rel_x < 0.5`) inserts at `hover`; the right
/// half, including the midpoint, inserts at `hover + 1`.
/// `None` when `hover >= len`, `len == 0`, or `rel_x` is non-finite / outside 0..=1.
pub fn pane_insert_slot(hover: usize, rel_x: f32, len: usize) -> Option<usize> {
    if len == 0 || hover >= len || !rel_x.is_finite() || !(0.0..=1.0).contains(&rel_x) {
        return None;
    }
    if rel_x >= 0.5 {
        Some(hover + 1)
    } else {
        Some(hover)
    }
}

/// Enough of a tab for [`detach_pane`] to move one pane without a window.
trait DetachTab {
    type Item;
    fn tab_id(&self) -> TabId;
    fn tree(&self) -> &Node;
    fn tree_mut(&mut self) -> &mut Node;
    fn pane_ids(&self) -> Vec<PaneId>;
    fn take_pane(&mut self, id: PaneId) -> Option<(usize, Self::Item)>;
    fn restore_pane(&mut self, index: usize, pane: Self::Item);
    fn set_focused(&mut self, id: PaneId);
    fn single(id: TabId, pane: Self::Item) -> Self;
    fn id_of(pane: &Self::Item) -> PaneId;
}

impl DetachTab for Tab {
    type Item = Pane;

    fn tab_id(&self) -> TabId {
        self.id
    }

    fn tree(&self) -> &Node {
        &self.tree
    }

    fn tree_mut(&mut self) -> &mut Node {
        &mut self.tree
    }

    fn pane_ids(&self) -> Vec<PaneId> {
        self.panes.iter().map(|pane| pane.id).collect()
    }

    fn take_pane(&mut self, id: PaneId) -> Option<(usize, Pane)> {
        let index = self.panes.iter().position(|pane| pane.id == id)?;
        Some((index, self.panes.remove(index)))
    }

    fn restore_pane(&mut self, index: usize, pane: Pane) {
        let index = index.min(self.panes.len());
        self.panes.insert(index, pane);
    }

    fn set_focused(&mut self, id: PaneId) {
        self.focused = id;
    }

    fn single(id: TabId, pane: Pane) -> Self {
        let focused = pane.id;
        Self {
            id,
            title: None,
            layout_id: None,
            tree: Node::Leaf(focused),
            focused,
            panes: vec![pane],
        }
    }

    fn id_of(pane: &Pane) -> PaneId {
        pane.id
    }
}

/// Pulls `pane` out of tab `source` into a new single-pane tab inserted at `slot`.
/// Returns the new tab's index. `None` leaves `tabs` unchanged, and the pane stays
/// in its source tab: a failed remove puts it back.
fn detach_pane<T: DetachTab>(
    tabs: &mut Vec<T>,
    source: TabId,
    pane: PaneId,
    slot: usize,
    new_id: TabId,
) -> Option<usize> {
    if slot > tabs.len() {
        return None;
    }
    let source_ix = tabs.iter().position(|tab| tab.tab_id() == source)?;
    let next_focus = {
        let tab = tabs.get(source_ix)?;
        let ids = tab.pane_ids();
        if ids.len() < 2 || !ids.contains(&pane) || !tab.tree().contains(pane) {
            return None;
        }
        let order = tab.tree().leaves();
        let at = order.iter().position(|id| *id == pane)?;
        if order.len() < 2 || tab.tree().clone().remove(pane).is_none() {
            return None;
        }
        if at > 0 { order[at - 1] } else { order[1] }
    };
    let tab = tabs.get_mut(source_ix)?;
    let (index, item) = tab.take_pane(pane)?;
    if T::id_of(&item) != pane {
        tab.restore_pane(index, item);
        return None;
    }
    // `remove` returns None only when this pane is the whole tree. The placeholder is that
    // same leaf, so putting the pane back leaves the tab as it was.
    let tree = std::mem::replace(tab.tree_mut(), Node::Leaf(pane));
    let Some(rest) = tree.remove(pane) else {
        tab.restore_pane(index, item);
        return None;
    };
    *tab.tree_mut() = rest;
    tab.set_focused(next_focus);
    tabs.insert(slot, T::single(new_id, item));
    Some(slot)
}

/// Which edge a pane drag should move onto. `None` for another tab, the pane under its own
/// pointer, a point outside 0..=1, or a non-finite coordinate.
pub fn pane_rearrange_hint(
    same_tab: bool,
    same_pane: bool,
    rel_x: f32,
    rel_y: f32,
) -> Option<Side> {
    if !same_tab
        || same_pane
        || !rel_x.is_finite()
        || !rel_y.is_finite()
        || !(0.0..=1.0).contains(&rel_x)
        || !(0.0..=1.0).contains(&rel_y)
    {
        return None;
    }
    Some(crate::split::drop_side(rel_x, rel_y))
}

/// Moves `source` onto `side` of `target` in this tab and focuses it. False, and the tab
/// unchanged, when the panes are the same, either is missing from the pane list, or
/// [`Node::move_pane`] refuses.
fn rearrange_pane<T: DetachTab>(tab: &mut T, source: PaneId, target: PaneId, side: Side) -> bool {
    let ids = tab.pane_ids();
    if !ids.contains(&source) || !ids.contains(&target) {
        return false;
    }
    if !tab.tree_mut().move_pane(source, target, side) {
        return false;
    }
    tab.set_focused(source);
    true
}

impl Shell {
    /// Hints a quarter only for another pane of this same tab. Self and other tabs stay blank,
    /// and a tab-merge hint is cleared so the two drags do not share one preview.
    pub(super) fn on_pane_drag_over_pane(
        &mut self,
        event: &DragMoveEvent<DraggedPane>,
        tab: TabId,
        pane: PaneId,
        cx: &mut Context<Self>,
    ) {
        if self.split_preview_kind == Some(super::split_drag::PreviewKind::TabMerge) {
            self.split_preview = None;
            self.split_preview_kind = None;
            cx.notify();
        }
        let (dragged_tab, dragged_pane) = {
            let dragged = event.drag(cx);
            (dragged.tab, dragged.pane)
        };
        let inside = super::split_drag::pane_rel(
            f32::from(event.event.position.x),
            f32::from(event.event.position.y),
            f32::from(event.bounds.origin.x),
            f32::from(event.bounds.origin.y),
            f32::from(event.bounds.size.width),
            f32::from(event.bounds.size.height),
        );
        if let Some((rel_x, rel_y)) = inside {
            let Some(side) =
                pane_rearrange_hint(dragged_tab == tab, dragged_pane == pane, rel_x, rel_y)
            else {
                if self
                    .split_preview
                    .is_some_and(|preview| preview.pane == pane && preview.tab == tab)
                {
                    self.split_preview = None;
                    self.split_preview_kind = None;
                    cx.notify();
                }
                return;
            };
            let next = super::split_drag::SplitPreview { tab, pane, side };
            if self.split_preview != Some(next)
                || self.split_preview_kind != Some(super::split_drag::PreviewKind::PaneRearrange)
            {
                self.split_preview = Some(next);
                self.split_preview_kind = Some(super::split_drag::PreviewKind::PaneRearrange);
                cx.notify();
            }
            return;
        }
        if self
            .split_preview
            .is_some_and(|preview| preview.pane == pane)
        {
            self.split_preview = None;
            self.split_preview_kind = None;
            cx.notify();
        }
    }

    /// Moves the dragged pane onto this one when the stored preview still names it.
    /// The side is that preview's, not a fresh read of the pointer.
    pub(super) fn accept_pane_rearrange(
        &mut self,
        dragged: &DraggedPane,
        tab: TabId,
        pane: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(preview) = self.split_preview.take() else {
            self.split_preview_kind = None;
            cx.notify();
            return;
        };
        self.split_preview_kind = None;
        if preview.tab != tab || preview.pane != pane || dragged.tab != tab {
            cx.notify();
            return;
        }
        let Some(host) = self.tabs.iter_mut().find(|candidate| candidate.id == tab) else {
            cx.notify();
            return;
        };
        if !rearrange_pane(host, dragged.pane, pane, preview.side) {
            cx.notify();
            return;
        }
        self.focus_front_pane(window, cx);
        self.persist_tabs();
    }
}

#[cfg(test)]
#[path = "shell_split_detach_tests.rs"]
mod tests;
