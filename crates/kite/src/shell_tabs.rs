//! Tab editing on the strip: renaming a title in place and dragging a tab to a new slot.

use gpui::{AppContext, Context, Entity, KeyDownEvent, Window};
use kite_ssh::ConnectSpec;

use super::{Pane, Shell, Tab};
use crate::i18n::fill;
use crate::session::Launch;
use crate::split::{self, Axis, Node};
use crate::tabs;
use crate::tabs_store::{
    CURRENT, Load, SavedAxis, SavedKind, SavedNode, SavedTab, SavedTabs, focus_among,
};
use crate::text_input::{InputColors, TextInput};

/// A title being edited: which tab, and the field holding the text.
pub(super) struct Rename {
    pub ix: usize,
    pub input: Entity<TextInput>,
}

impl Shell {
    pub(crate) fn is_renaming(&self, ix: usize) -> bool {
        self.renaming.as_ref().is_some_and(|r| r.ix == ix)
    }

    /// Swaps the tab's title for a field holding it, selected for typing over.
    pub(crate) fn start_rename(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        let current = tab.strip_label();
        let baseline = tab.generated_label();
        let t = self.theme;
        let colors = InputColors {
            text: t.text,
            placeholder: t.faint,
            cursor: t.accent,
            selection: t.accent.opacity(0.35),
        };
        let input = cx.new(|cx| {
            let mut input = TextInput::new(baseline, false, colors, cx);
            input.set_text(current, cx);
            input.select_everything(cx);
            input
        });
        window.focus(&gpui::Focusable::focus_handle(input.read(cx), cx), cx);
        self.renaming = Some(Rename { ix, input });
        cx.notify();
    }

    /// Saves the field. An empty title, or the alias itself, clears the custom title.
    pub(crate) fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.renaming.take() else {
            return;
        };
        let typed = rename.input.read(cx).text().trim().to_owned();
        if let Some(tab) = self.tabs.get_mut(rename.ix) {
            // Unchanged means the generated strip text, so committing "prod · 2" does not
            // store that count as a custom title.
            let baseline = tab.generated_label();
            tab.title = custom_title(&typed, &baseline);
        }
        self.persist_tabs();
        self.restore_focus(window, cx);
    }

    pub(crate) fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming.take().is_some() {
            self.restore_focus(window, cx);
        }
    }

    /// Enter saves and Escape drops the edit; true when the key was for the field.
    pub(super) fn on_rename_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.renaming.is_none() {
            return false;
        }
        match event.keystroke.key.as_str() {
            "enter" => self.commit_rename(window, cx),
            "escape" => self.cancel_rename(window, cx),
            _ => return false,
        }
        true
    }

    /// A tab was dropped on slot `to`; the tab in front stays in front.
    pub(crate) fn move_tab(
        &mut self,
        from: usize,
        to: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.renaming = None;
        let active = tabs::reorder(&mut self.tabs, self.active, from, to);
        self.activate_tab(active, window, cx);
    }
}

impl Shell {
    pub(crate) fn start_logging(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(session) = self.tabs.get(ix).map(|t| t.session().clone()) else {
            return;
        };
        match session.update(cx, |s, cx| {
            cx.notify();
            s.start_logging()
        }) {
            // The file name, not the full path (it ran past the toast), and a way to find it.
            Ok(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.toast(
                    super::Toast::new(super::ToastKind::Default, fill("Logging to {}", &[&name]))
                        .action(crate::platform::REVEAL_LABEL, move |_, _, cx| {
                            cx.reveal_path(&path)
                        }),
                    cx,
                );
            }
            Err(e) => self.notify_toast(
                super::ToastKind::Critical,
                fill("Could not start the log: {}", &[&e.to_string()]),
                cx,
            ),
        }
    }

    pub(crate) fn stop_logging(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(session) = self.tabs.get(ix).map(|t| t.session().clone()) else {
            return;
        };
        let path = session.update(cx, |s, cx| {
            cx.notify();
            s.stop_logging()
        });
        if let Some(path) = path {
            self.notify_toast(
                super::ToastKind::Default,
                fill("Log saved to {}", &[&path.display().to_string()]),
                cx,
            );
        }
    }

    /// Writes the open tabs to `tabs.json`. A failed write is logged, not fatal. Skipped while
    /// restoring, when reopening is off, and when the file on disk is newer or corrupt.
    pub(crate) fn persist_tabs(&self) {
        if self.restoring || !self.settings.reopen_tabs || self.tabs_unreadable {
            return;
        }
        let Some(dir) = crate::settings::dir() else {
            return;
        };
        if let Err(e) = self.saved_tabs().save(&dir) {
            tracing::warn!(error = %e, "tabs_save_failed");
        }
    }

    fn saved_tabs(&self) -> SavedTabs {
        SavedTabs {
            version: CURRENT,
            tabs: self.tabs.iter().map(saved_tab).collect(),
            active: self.active,
        }
    }

    /// Reopens the last run's tabs. Every pane waits for Enter, so a split does not dial
    /// every server at launch. With reopening off, tabs stay closed, but a newer or corrupt
    /// file is still remembered so a later change cannot replace it.
    pub(crate) fn restore_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = crate::settings::dir() else {
            return;
        };
        let loaded = SavedTabs::load(&dir);
        if !loaded.may_write() {
            self.tabs_unreadable = true;
            return;
        }
        if !self.settings.reopen_tabs {
            return;
        }
        let Load::Ready(saved) = loaded else {
            return;
        };
        let saved = saved.sanitize(|alias| {
            let keep = self.alias_keepable(alias);
            if !keep {
                tracing::warn!(alias, "tab_restore_skipped");
            }
            keep
        });
        self.restoring = true;
        let mut skipped_before_active = 0;
        for (ix, saved_tab) in saved.tabs.iter().enumerate() {
            if !self.restore_saved_tab(saved_tab, window, cx) && ix < saved.active {
                skipped_before_active += 1;
            }
        }
        if !self.tabs.is_empty() {
            let saved = SavedTabs {
                active: saved.active.saturating_sub(skipped_before_active),
                ..saved
            };
            let active = saved.active_slot().min(self.tabs.len() - 1);
            self.activate_tab(active, window, cx);
        }
        self.restoring = false;
    }

    /// One tab, with a pane per leaf. False when every host was skipped.
    fn restore_saved_tab(
        &mut self,
        saved: &SavedTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(node) = saved_layout(saved) else {
            return false;
        };
        let mut opened = Vec::new();
        self.open_saved_leaves(&node, &mut opened, window, cx);
        let kept: Vec<bool> = opened.iter().map(|pane| pane.is_some()).collect();
        let Some(focus) = focus_among(saved.focused, &kept) else {
            return false;
        };
        let Some((tree, panes)) = assemble(&node, &mut opened.into_iter()) else {
            return false;
        };
        let Some(&focused) = tree.leaves().get(focus) else {
            return false;
        };
        self.next_tab += 1;
        self.tabs.push(Tab {
            id: self.next_tab,
            title: saved.title.clone(),
            tree,
            focused,
            panes,
        });
        true
    }

    fn open_saved_leaves(
        &mut self,
        node: &SavedNode,
        out: &mut Vec<Option<Pane>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if node.is_split() {
            if let Some(first) = &node.first {
                self.open_saved_leaves(first, out, window, cx);
            }
            if let Some(second) = &node.second {
                self.open_saved_leaves(second, out, window, cx);
            }
            return;
        }
        let Some(alias) = node.alias.as_deref() else {
            return;
        };
        out.push(self.open_idle_pane(alias, window, cx));
    }

    /// A connection that does not dial yet. `None` when this host cannot be opened.
    fn open_idle_pane(
        &mut self,
        alias: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Pane> {
        if is_local_alias(alias) {
            let label = crate::i18n::tr("Local Terminal");
            return Some(self.make_pane(Launch::Local, label, window, cx));
        }
        let spec = match self.hosts.iter().find(|host| host.alias == alias) {
            Some(host) => ConnectSpec::from_host_entry(host),
            None => ConnectSpec::parse(alias),
        };
        match spec {
            Ok(spec) => Some(self.make_pane(Launch::SshIdle(spec), alias, window, cx)),
            Err(e) => {
                tracing::warn!(alias, error = %e, "tab_restore_skipped");
                None
            }
        }
    }

    /// In the host list, or a target `ConnectSpec::parse` accepts. Anything else is dropped.
    fn alias_keepable(&self, alias: &str) -> bool {
        is_local_alias(alias)
            || self.hosts.iter().any(|host| host.alias == alias)
            || ConnectSpec::parse(alias).is_ok()
    }
}

fn is_local_alias(alias: &str) -> bool {
    alias == "Local Terminal" || alias == "本地终端"
}

fn saved_tab(tab: &Tab) -> SavedTab {
    let leaves = tab.tree.leaves();
    let focused = leaves.iter().position(|id| *id == tab.focused).unwrap_or(0);
    SavedTab {
        kind: SavedKind::Ssh {
            alias: tab.focused_alias().to_owned(),
        },
        title: tab.title.clone(),
        tree: Some(saved_node(&tab.tree, &tab.panes)),
        focused,
    }
}

fn saved_layout(saved: &SavedTab) -> Option<SavedNode> {
    if let Some(tree) = saved.tree.clone() {
        return Some(tree);
    }
    match &saved.kind {
        SavedKind::Ssh { alias } => Some(SavedNode::leaf(alias.clone())),
        SavedKind::Unknown => None,
    }
}

fn saved_node(node: &Node, panes: &[Pane]) -> SavedNode {
    match node {
        Node::Leaf(id) => {
            let alias = panes
                .iter()
                .find(|pane| pane.id == *id)
                .map(|pane| pane.alias.clone())
                .unwrap_or_default();
            SavedNode::leaf(alias)
        }
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let axis = match axis {
                Axis::Row => SavedAxis::Row,
                Axis::Column => SavedAxis::Column,
            };
            SavedNode::split(
                axis,
                *ratio,
                saved_node(first, panes),
                saved_node(second, panes),
            )
        }
    }
}

fn assemble<I>(node: &SavedNode, panes: &mut I) -> Option<(Node, Vec<Pane>)>
where
    I: Iterator<Item = Option<Pane>>,
{
    if node.is_split() {
        let axis = match node.axis {
            Some(SavedAxis::Column) => Axis::Column,
            _ => Axis::Row,
        };
        let ratio = split::clamp_ratio(node.ratio.unwrap_or(0.5));
        let first = node
            .first
            .as_deref()
            .and_then(|child| assemble(child, panes));
        let second = node
            .second
            .as_deref()
            .and_then(|child| assemble(child, panes));
        return match (first, second) {
            (Some((first, mut panes_a)), Some((second, panes_b))) => {
                panes_a.extend(panes_b);
                Some((
                    Node::Split {
                        axis,
                        ratio,
                        first: Box::new(first),
                        second: Box::new(second),
                    },
                    panes_a,
                ))
            }
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        };
    }
    node.alias.as_ref()?;
    let pane = panes.next().flatten()?;
    let id = pane.id;
    Some((Node::Leaf(id), vec![pane]))
}

/// What a typed title means: nothing, or the alias it already shows, is no custom title.
fn custom_title(typed: &str, alias: &str) -> Option<String> {
    (!typed.is_empty() && typed != alias).then(|| typed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::custom_title;

    #[test]
    fn empty_or_unchanged_titles_are_not_custom() {
        assert_eq!(custom_title("", "prod"), None);
        assert_eq!(custom_title("prod", "prod"), None);
        assert_eq!(custom_title("prod db", "prod"), Some("prod db".into()));
    }
}
