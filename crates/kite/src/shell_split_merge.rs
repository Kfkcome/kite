//! Each pane keeps the host it was opened with. A tab has no alias of its own: the strip,
//! the sidebar and reconnect ask the pane that has the keyboard, or the pane that matches.
//!
//! Dropping a single-pane tab on a pane of another tab moves that session into the target.

use gpui::{App, Context, Window};

use crate::indicator::Dot;
use crate::split::{Node, PaneId, Side};
use crate::tabs::{DraggedTab, TabInfo};

use super::broadcast_ui::TabId;
use super::{Pane, Shell, Tab};

impl Tab {
    /// Alias of the focused pane, or of the first pane when focus names nothing.
    pub(super) fn focused_alias(&self) -> &str {
        match self.panes.iter().find(|pane| pane.id == self.focused) {
            Some(pane) => pane.alias.as_str(),
            None => match self.panes.first() {
                Some(pane) => pane.alias.as_str(),
                None => "",
            },
        }
    }

    /// Whether any pane in this tab was opened as `alias`.
    pub(super) fn has_host(&self, alias: &str) -> bool {
        self.panes.iter().any(|pane| pane.alias == alias)
    }

    /// The first pane in this tab whose host is `alias`.
    pub(super) fn pane_for_host(&self, alias: &str) -> Option<crate::split::PaneId> {
        let aliases = self.pane_aliases();
        let (_, pane_ix) = first_host_pane(std::slice::from_ref(&aliases), alias)?;
        self.panes.get(pane_ix).map(|pane| pane.id)
    }

    /// Text for the tab strip, including a custom title when the user set one.
    pub(super) fn strip_label(&self) -> String {
        let aliases = self.pane_aliases();
        tab_strip_label(self.title.as_deref(), self.focused_alias(), &aliases)
    }

    /// Strip text as if the tab had no custom title.
    pub(super) fn generated_label(&self) -> String {
        let aliases = self.pane_aliases();
        tab_strip_label(None, self.focused_alias(), &aliases)
    }

    fn pane_aliases(&self) -> Vec<&str> {
        self.panes.iter().map(|pane| pane.alias.as_str()).collect()
    }
}

/// Enough of a tab for [`merge_panes`] to move one pane without a window.
trait MergeTab {
    type Item;
    fn tree(&self) -> &Node;
    fn set_tree(&mut self, tree: Node);
    fn only_pane_id(&self) -> Option<PaneId>;
    fn take_only_pane(&mut self) -> Option<Self::Item>;
    fn restore_only_pane(&mut self, pane: Self::Item);
    fn push_pane(&mut self, pane: Self::Item);
    fn set_focused(&mut self, id: PaneId);
    fn id_of(pane: &Self::Item) -> PaneId;
}

impl MergeTab for Tab {
    type Item = Pane;

    fn tree(&self) -> &Node {
        &self.tree
    }

    fn set_tree(&mut self, tree: Node) {
        self.tree = tree;
    }

    fn only_pane_id(&self) -> Option<PaneId> {
        let [pane] = self.panes.as_slice() else {
            return None;
        };
        Some(pane.id)
    }

    fn take_only_pane(&mut self) -> Option<Pane> {
        if self.panes.len() == 1 {
            self.panes.pop()
        } else {
            None
        }
    }

    fn restore_only_pane(&mut self, pane: Pane) {
        self.panes.push(pane);
    }

    fn push_pane(&mut self, pane: Pane) {
        self.panes.push(pane);
    }

    fn set_focused(&mut self, id: PaneId) {
        self.focused = id;
    }

    fn id_of(pane: &Pane) -> PaneId {
        pane.id
    }
}

impl Shell {
    /// The first open pane for `alias`: tabs in order, then each tab's pane list.
    /// A later tab is not used when an earlier one already has that host.
    pub(super) fn locate_host(&self, alias: &str) -> Option<(usize, crate::split::PaneId)> {
        self.tabs.iter().enumerate().find_map(|(ix, tab)| {
            if !tab.has_host(alias) {
                None
            } else {
                tab.pane_for_host(alias).map(|id| (ix, id))
            }
        })
    }

    pub(super) fn tab_infos(&self, cx: &App) -> Vec<TabInfo> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(ix, tab)| {
                let hosts = tab
                    .panes
                    .iter()
                    .map(|pane| {
                        let session = pane.session.read(cx);
                        (
                            pane.alias.clone(),
                            Dot::of(&session.status, session.is_asking()),
                        )
                    })
                    .collect();
                let session = tab.session().read(cx);
                TabInfo {
                    alias: tab.focused_alias().to_owned(),
                    hosts,
                    strip_label: tab.strip_label(),
                    logging: session.is_logging(),
                    broadcast: self.is_broadcasting(tab.id),
                    dot: Dot::of(&session.status, session.is_asking()),
                    rename: self
                        .renaming
                        .as_ref()
                        .filter(|rename| rename.ix == ix)
                        .map(|rename| rename.input.clone()),
                }
            })
            .collect()
    }
}

/// Strip text: a custom `title` wins unchanged; one host is that alias; several hosts are
/// `"{focused_alias} · {pane_count}"` (every pane, not the number of distinct hosts).
pub(super) fn tab_strip_label(
    title: Option<&str>,
    focused_alias: &str,
    pane_aliases: &[&str],
) -> String {
    if let Some(title) = title {
        return title.to_owned();
    }
    let distinct = pane_aliases
        .iter()
        .enumerate()
        .filter(|(ix, alias)| !pane_aliases[..*ix].contains(alias))
        .count();
    if distinct <= 1 {
        if let Some(alias) = pane_aliases.first() {
            return (*alias).to_owned();
        }
        return focused_alias.to_owned();
    }
    format!("{focused_alias} · {}", pane_aliases.len())
}

/// First `(tab, pane)` whose alias is `alias`, walking tabs then panes. `None` when none match.
pub(super) fn first_host_pane(tabs: &[Vec<&str>], alias: &str) -> Option<(usize, usize)> {
    tabs.iter().enumerate().find_map(|(tab_ix, panes)| {
        panes
            .iter()
            .position(|pane| *pane == alias)
            .map(|pane_ix| (tab_ix, pane_ix))
    })
}

/// The slot of `target` after removing `source`. `None` when the indices are
/// equal or either is invalid for a list of `len` tabs.
pub(super) fn active_after_merge(source: usize, target: usize, len: usize) -> Option<usize> {
    if source == target || source >= len || target >= len {
        None
    } else if source < target {
        Some(target - 1)
    } else {
        Some(target)
    }
}

/// Moves the only pane of `tabs[source]` onto `side` of `target_pane` in `tabs[target]`.
/// Returns where `target` sits afterwards. `None` leaves `tabs` unchanged.
fn merge_panes<T: MergeTab>(
    tabs: &mut Vec<T>,
    source: usize,
    target: usize,
    target_pane: PaneId,
    side: Side,
) -> Option<usize> {
    let active = active_after_merge(source, target, tabs.len())?;
    // Try the insert on a copy before the pane leaves its tab, so a rejected drop cannot
    // drop the session.
    let (moving, next) = {
        let moving = tabs.get(source)?.only_pane_id()?;
        let target_tab = tabs.get(target)?;
        if !target_tab.tree().contains(target_pane) || target_tab.tree().contains(moving) {
            return None;
        }
        let mut next = target_tab.tree().clone();
        if !next.insert(target_pane, side, moving) {
            return None;
        }
        (moving, next)
    };
    let pane = tabs.get_mut(source).and_then(MergeTab::take_only_pane)?;
    if T::id_of(&pane) != moving {
        if let Some(src) = tabs.get_mut(source) {
            src.restore_only_pane(pane);
        }
        return None;
    }
    let Some(dest) = tabs.get_mut(target) else {
        if let Some(src) = tabs.get_mut(source) {
            src.restore_only_pane(pane);
        }
        return None;
    };
    dest.set_tree(next);
    dest.set_focused(moving);
    dest.push_pane(pane);
    tabs.remove(source);
    Some(active)
}

impl Shell {
    /// Moves the dragged tab's pane onto this one when the stored preview still names it.
    /// The side is that preview's, not a fresh read of the pointer.
    pub(super) fn accept_tab_drop(
        &mut self,
        dragged: &DraggedTab,
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
        if preview.tab != tab || preview.pane != pane {
            cx.notify();
            return;
        }
        let Some(target) = self.tabs.iter().position(|candidate| candidate.id == tab) else {
            cx.notify();
            return;
        };
        let Some(active) = merge_panes(&mut self.tabs, dragged.ix, target, pane, preview.side)
        else {
            cx.notify();
            return;
        };
        // Slots moved, so a half-typed title would land on the wrong tab.
        self.renaming = None;
        self.sync_broadcast(cx);
        self.show_tab(active, window, cx);
        self.close_orphan_panels(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{MergeTab, active_after_merge, first_host_pane, merge_panes, tab_strip_label};
    use crate::split::{Axis, Node, PaneId, Side};

    #[test]
    fn a_custom_title_wins_unchanged() {
        assert_eq!(
            tab_strip_label(Some("Mine"), "prod", &["prod", "staging"]),
            "Mine"
        );
        assert_eq!(tab_strip_label(Some("prod"), "prod", &["prod"]), "prod");
        assert_eq!(tab_strip_label(Some(""), "prod", &["prod"]), "");
    }

    #[test]
    fn one_host_is_just_the_alias_even_when_split() {
        assert_eq!(tab_strip_label(None, "prod", &["prod"]), "prod");
        assert_eq!(tab_strip_label(None, "prod", &["prod", "prod"]), "prod");
        assert_eq!(tab_strip_label(None, "prod", &[]), "prod");
    }

    #[test]
    fn several_hosts_show_the_focused_alias_and_the_pane_count() {
        assert_eq!(
            tab_strip_label(None, "prod", &["prod", "staging"]),
            "prod · 2"
        );
        assert_eq!(
            tab_strip_label(None, "staging", &["prod", "staging"]),
            "staging · 2"
        );
        assert_eq!(
            tab_strip_label(None, "prod", &["prod", "staging", "prod"]),
            "prod · 3"
        );
        assert_eq!(
            tab_strip_label(None, "dev", &["prod", "staging", "dev"]),
            "dev · 3"
        );
    }

    #[test]
    fn a_host_in_a_multi_host_tab_is_found_and_the_first_tab_wins() {
        let tabs = vec![
            vec!["prod", "staging"],
            vec!["dev"],
            vec!["staging", "prod"],
        ];
        assert_eq!(first_host_pane(&tabs, "staging"), Some((0, 1)));
        assert_eq!(first_host_pane(&tabs, "prod"), Some((0, 0)));
        assert_eq!(first_host_pane(&tabs, "dev"), Some((1, 0)));
        assert_eq!(first_host_pane(&tabs, "missing"), None);
        assert_eq!(first_host_pane(&[], "prod"), None);
    }

    #[test]
    fn the_target_slot_shifts_only_when_the_source_sat_before_it() {
        assert_eq!(active_after_merge(0, 2, 3), Some(1));
        assert_eq!(active_after_merge(2, 0, 3), Some(0));
        assert_eq!(active_after_merge(1, 2, 3), Some(1));
        assert_eq!(active_after_merge(1, 1, 3), None);
        assert_eq!(active_after_merge(0, 3, 3), None);
        assert_eq!(active_after_merge(3, 0, 3), None);
        assert_eq!(active_after_merge(0, 0, 0), None);
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct HostPane {
        id: PaneId,
        alias: &'static str,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Body {
        tree: Node,
        focused: PaneId,
        panes: Vec<HostPane>,
    }

    fn leaf(id: PaneId, alias: &'static str) -> Body {
        Body {
            tree: Node::Leaf(id),
            focused: id,
            panes: vec![HostPane { id, alias }],
        }
    }

    impl MergeTab for Body {
        type Item = HostPane;

        fn tree(&self) -> &Node {
            &self.tree
        }

        fn set_tree(&mut self, tree: Node) {
            self.tree = tree;
        }

        fn only_pane_id(&self) -> Option<PaneId> {
            let [pane] = self.panes.as_slice() else {
                return None;
            };
            Some(pane.id)
        }

        fn take_only_pane(&mut self) -> Option<HostPane> {
            if self.panes.len() == 1 {
                self.panes.pop()
            } else {
                None
            }
        }

        fn restore_only_pane(&mut self, pane: HostPane) {
            self.panes.push(pane);
        }

        fn push_pane(&mut self, pane: HostPane) {
            self.panes.push(pane);
        }

        fn set_focused(&mut self, id: PaneId) {
            self.focused = id;
        }

        fn id_of(pane: &HostPane) -> PaneId {
            pane.id
        }
    }

    #[test]
    fn merging_moves_the_only_pane_and_focuses_it_in_the_new_slot() {
        let mut tabs = vec![leaf(1, "prod"), leaf(2, "staging"), leaf(3, "dev")];
        assert_eq!(merge_panes(&mut tabs, 0, 2, 3, Side::Left), Some(1));
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0], leaf(2, "staging"));
        assert_eq!(tabs[1].focused, 1);
        assert_eq!(
            tabs[1].panes,
            vec![
                HostPane {
                    id: 3,
                    alias: "dev"
                },
                HostPane {
                    id: 1,
                    alias: "prod"
                },
            ]
        );
        let mut expected = Node::Leaf(3);
        assert!(expected.insert(3, Side::Left, 1));
        assert_eq!(tabs[1].tree, expected);

        let mut split = Node::Leaf(1);
        assert!(split.insert(1, Side::Right, 2));
        let mut tabs = vec![
            Body {
                tree: split,
                focused: 1,
                panes: vec![
                    HostPane {
                        id: 1,
                        alias: "prod",
                    },
                    HostPane {
                        id: 2,
                        alias: "staging",
                    },
                ],
            },
            leaf(4, "dev"),
        ];
        assert_eq!(merge_panes(&mut tabs, 1, 0, 2, Side::Bottom), Some(0));
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].focused, 4);
        assert_eq!(
            tabs[0].panes,
            vec![
                HostPane {
                    id: 1,
                    alias: "prod"
                },
                HostPane {
                    id: 2,
                    alias: "staging"
                },
                HostPane {
                    id: 4,
                    alias: "dev"
                },
            ]
        );
        let mut expected = Node::Leaf(1);
        assert!(expected.insert(1, Side::Right, 2));
        assert!(expected.insert(2, Side::Bottom, 4));
        assert_eq!(tabs[0].tree, expected);
    }

    #[test]
    fn a_bad_merge_leaves_the_tabs_where_they_were() {
        let mut tabs = vec![leaf(1, "prod"), leaf(2, "staging"), leaf(3, "dev")];
        tabs[0].panes.push(HostPane {
            id: 9,
            alias: "prod",
        });
        assert!(tabs[0].tree.split(1, Axis::Row, 9));
        let before = tabs.clone();
        assert_eq!(merge_panes(&mut tabs, 0, 2, 3, Side::Right), None);
        assert_eq!(tabs, before);

        let mut tabs = vec![leaf(1, "prod"), leaf(2, "staging")];
        let before = tabs.clone();
        assert_eq!(merge_panes(&mut tabs, 0, 0, 1, Side::Right), None);
        assert_eq!(merge_panes(&mut tabs, 1, 0, 99, Side::Left), None);
        assert_eq!(merge_panes(&mut tabs, 3, 0, 1, Side::Left), None);
        assert_eq!(merge_panes(&mut tabs, 0, 4, 2, Side::Top), None);
        assert_eq!(tabs, before);

        let mut tabs = vec![leaf(1, "prod"), leaf(1, "staging")];
        let before = tabs.clone();
        assert_eq!(merge_panes(&mut tabs, 0, 1, 1, Side::Right), None);
        assert_eq!(tabs, before);
    }
}
