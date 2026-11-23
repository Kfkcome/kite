use super::{DetachTab, detach_pane, pane_insert_slot, pane_rearrange_hint, rearrange_pane};
use crate::split::{Node, PaneId, Side};

#[test]
fn the_left_half_inserts_before_the_pill_and_the_midpoint_inserts_after() {
    assert_eq!(pane_insert_slot(0, 0.25, 3), Some(0));
    assert_eq!(pane_insert_slot(0, 0.75, 3), Some(1));
    assert_eq!(pane_insert_slot(2, 0.9, 3), Some(3));
    assert_eq!(pane_insert_slot(1, 0.5, 3), Some(2));
    assert_eq!(pane_insert_slot(1, 0.0, 3), Some(1));
    assert_eq!(pane_insert_slot(1, 1.0, 3), Some(2));
}

#[test]
fn a_slot_outside_the_strip_or_a_non_finite_pointer_is_none() {
    assert_eq!(pane_insert_slot(3, 0.25, 3), None);
    assert_eq!(pane_insert_slot(0, 0.25, 0), None);
    assert_eq!(pane_insert_slot(0, -0.01, 3), None);
    assert_eq!(pane_insert_slot(0, 1.01, 3), None);
    assert_eq!(pane_insert_slot(0, f32::NAN, 3), None);
    assert_eq!(pane_insert_slot(0, f32::INFINITY, 3), None);
    assert_eq!(pane_insert_slot(0, f32::NEG_INFINITY, 3), None);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HostPane {
    id: PaneId,
    alias: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
struct Body {
    id: u64,
    title: Option<&'static str>,
    tree: Node,
    focused: PaneId,
    panes: Vec<HostPane>,
}

fn leaf(id: u64, pane: PaneId, alias: &'static str) -> Body {
    Body {
        id,
        title: None,
        tree: Node::Leaf(pane),
        focused: pane,
        panes: vec![HostPane { id: pane, alias }],
    }
}

fn pair(id: u64, title: Option<&'static str>) -> Body {
    let mut tree = Node::Leaf(1);
    assert!(tree.insert(1, Side::Right, 2));
    Body {
        id,
        title,
        tree,
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
    }
}

impl DetachTab for Body {
    type Item = HostPane;

    fn tab_id(&self) -> u64 {
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

    fn take_pane(&mut self, id: PaneId) -> Option<(usize, HostPane)> {
        let index = self.panes.iter().position(|pane| pane.id == id)?;
        Some((index, self.panes.remove(index)))
    }

    fn restore_pane(&mut self, index: usize, pane: HostPane) {
        let index = index.min(self.panes.len());
        self.panes.insert(index, pane);
    }

    fn set_focused(&mut self, id: PaneId) {
        self.focused = id;
    }

    fn single(id: u64, pane: HostPane) -> Self {
        let focused = pane.id;
        Self {
            id,
            title: None,
            tree: Node::Leaf(focused),
            focused,
            panes: vec![pane],
        }
    }

    fn id_of(pane: &HostPane) -> PaneId {
        pane.id
    }
}

#[test]
fn detaching_before_the_source_inserts_there_and_focuses_the_leaf_before() {
    let mut tabs = vec![
        leaf(10, 9, "dev"),
        pair(20, Some("Keep")),
        leaf(30, 8, "ops"),
    ];
    assert_eq!(detach_pane(&mut tabs, 20, 2, 1, 40), Some(1));
    assert_eq!(tabs[0], leaf(10, 9, "dev"));
    assert_eq!(tabs[1], leaf(40, 2, "staging"));
    assert_eq!(tabs[2].id, 20);
    assert_eq!(tabs[2].title, Some("Keep"));
    assert_eq!(tabs[2].focused, 1);
    assert_eq!(tabs[2].tree, Node::Leaf(1));
    assert_eq!(
        tabs[2].panes,
        vec![HostPane {
            id: 1,
            alias: "prod"
        }]
    );
    assert_eq!(tabs[3], leaf(30, 8, "ops"));
}

#[test]
fn detaching_after_the_source_inserts_there_and_focuses_the_following_leaf() {
    let mut tabs = vec![
        leaf(10, 9, "dev"),
        pair(20, Some("Keep")),
        leaf(30, 8, "ops"),
    ];
    assert_eq!(detach_pane(&mut tabs, 20, 1, 2, 40), Some(2));
    assert_eq!(tabs[0], leaf(10, 9, "dev"));
    assert_eq!(tabs[1].focused, 2);
    assert_eq!(tabs[1].tree, Node::Leaf(2));
    assert_eq!(tabs[1].title, Some("Keep"));
    assert_eq!(
        tabs[1].panes,
        vec![HostPane {
            id: 2,
            alias: "staging"
        }]
    );
    assert_eq!(tabs[2], leaf(40, 1, "prod"));
    assert_eq!(tabs[3], leaf(30, 8, "ops"));
}

#[test]
fn detaching_at_zero_inserts_in_front_of_every_tab() {
    let mut tabs = vec![leaf(10, 9, "dev"), pair(20, None), leaf(30, 8, "ops")];
    assert_eq!(detach_pane(&mut tabs, 20, 2, 0, 40), Some(0));
    assert_eq!(tabs[0], leaf(40, 2, "staging"));
    assert_eq!(tabs[1], leaf(10, 9, "dev"));
    assert_eq!(tabs[2].id, 20);
    assert_eq!(tabs[2].focused, 1);
    assert_eq!(tabs[3], leaf(30, 8, "ops"));
}

#[test]
fn detaching_at_the_end_appends() {
    let mut tabs = vec![leaf(10, 9, "dev"), pair(20, None), leaf(30, 8, "ops")];
    assert_eq!(detach_pane(&mut tabs, 20, 2, 3, 40), Some(3));
    assert_eq!(tabs[3], leaf(40, 2, "staging"));
    assert_eq!(tabs[1].id, 20);
    assert_eq!(tabs[1].focused, 1);
    assert_eq!(tabs.len(), 4);
}

#[test]
fn detaching_a_middle_pane_focuses_the_leaf_before_it() {
    let mut tree = Node::Leaf(1);
    assert!(tree.insert(1, Side::Right, 2));
    assert!(tree.insert(2, Side::Bottom, 3));
    let mut tabs = vec![Body {
        id: 1,
        title: None,
        tree,
        focused: 3,
        panes: vec![
            HostPane { id: 1, alias: "a" },
            HostPane { id: 2, alias: "b" },
            HostPane { id: 3, alias: "c" },
        ],
    }];
    assert_eq!(detach_pane(&mut tabs, 1, 2, 1, 9), Some(1));
    assert_eq!(tabs[0].focused, 1);
    let mut expected = Node::Leaf(1);
    assert!(expected.insert(1, Side::Right, 3));
    assert_eq!(tabs[0].tree, expected);
    assert_eq!(
        tabs[0].panes.iter().map(|pane| pane.id).collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(tabs[1], leaf(9, 2, "b"));
}

#[test]
fn a_one_pane_source_a_missing_pane_or_a_bad_slot_changes_nothing() {
    let mut tabs = vec![
        leaf(10, 9, "dev"),
        pair(20, Some("Keep")),
        leaf(30, 8, "ops"),
    ];
    let before = tabs.clone();
    assert_eq!(detach_pane(&mut tabs, 10, 9, 0, 40), None);
    assert_eq!(detach_pane(&mut tabs, 20, 9, 1, 40), None);
    assert_eq!(detach_pane(&mut tabs, 99, 2, 1, 40), None);
    assert_eq!(detach_pane(&mut tabs, 20, 2, 4, 40), None);
    assert_eq!(detach_pane(&mut tabs, 20, 2, usize::MAX, 40), None);
    assert_eq!(tabs, before);
}

#[test]
fn a_same_tab_drag_hints_the_quarter_and_self_or_another_tab_does_not() {
    assert_eq!(pane_rearrange_hint(true, false, 0.1, 0.5), Some(Side::Left));
    assert_eq!(
        pane_rearrange_hint(true, false, 0.9, 0.5),
        Some(Side::Right)
    );
    assert_eq!(pane_rearrange_hint(true, false, 0.5, 0.1), Some(Side::Top));
    assert_eq!(
        pane_rearrange_hint(true, false, 0.5, 0.9),
        Some(Side::Bottom)
    );
    assert_eq!(
        pane_rearrange_hint(true, false, 0.8, 0.6),
        Some(Side::Right)
    );
    assert_eq!(pane_rearrange_hint(true, true, 0.1, 0.5), None);
    assert_eq!(pane_rearrange_hint(true, true, 0.5, 0.9), None);
    assert_eq!(pane_rearrange_hint(false, false, 0.1, 0.5), None);
    assert_eq!(pane_rearrange_hint(false, false, 0.5, 0.9), None);
    assert_eq!(pane_rearrange_hint(false, true, 0.9, 0.5), None);
}

#[test]
fn a_non_finite_or_out_of_range_rearrange_does_not_hint() {
    assert_eq!(pane_rearrange_hint(true, false, f32::NAN, 0.5), None);
    assert_eq!(pane_rearrange_hint(true, false, 0.1, f32::NAN), None);
    assert_eq!(pane_rearrange_hint(true, false, f32::INFINITY, 0.5), None);
    assert_eq!(
        pane_rearrange_hint(true, false, 0.5, f32::NEG_INFINITY),
        None
    );
    assert_eq!(pane_rearrange_hint(true, false, -0.1, 0.5), None);
    assert_eq!(pane_rearrange_hint(true, false, 1.1, 0.5), None);
    assert_eq!(pane_rearrange_hint(true, false, 0.5, -0.1), None);
    assert_eq!(pane_rearrange_hint(true, false, 0.5, 1.1), None);
}

#[test]
fn rearranging_focuses_the_moved_pane_and_a_drop_on_itself_changes_nothing() {
    let mut tab = pair(1, None);
    assert!(rearrange_pane(&mut tab, 2, 1, Side::Left));
    assert_eq!(tab.focused, 2);
    assert_eq!(tab.panes.len(), 2);
    let mut expected = Node::Leaf(2);
    assert!(expected.insert(2, Side::Right, 1));
    assert_eq!(tab.tree, expected);
    let before = tab.clone();
    assert!(!rearrange_pane(&mut tab, 2, 2, Side::Right));
    assert!(!rearrange_pane(&mut tab, 9, 1, Side::Left));
    assert_eq!(tab, before);
}
