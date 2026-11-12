//! The panes of one tab as a binary tree: a leaf is a pane, a split holds two subtrees side
//! by side or one above the other and remembers how the space is shared. Closing a pane
//! collapses its parent, so the sibling takes the space. Pure data, so the rules are tested
//! without a window.

/// Which way a split divides its area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// First on the left, second on the right.
    Row,
    /// First on top, second below.
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Which edge of a target pane an inserted or moved pane sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

pub type PaneId = u32;

/// Smallest fraction of a split given to `first`, so a pane cannot be squeezed away.
pub const RATIO_MIN: f32 = 0.15;
/// Largest fraction of a split given to `first`.
pub const RATIO_MAX: f32 = 0.85;

const DEFAULT_RATIO: f32 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Leaf(PaneId),
    Split {
        axis: Axis,
        /// Fraction of this split's area given to `first` (left or top).
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

/// A pane's place in the unit square, as fractions of the tab's area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A non-finite value becomes 0.5 before clamping, because `f32::clamp` leaves NaN as NaN.
pub(crate) fn clamp_ratio(ratio: f32) -> f32 {
    let ratio = if ratio.is_finite() {
        ratio
    } else {
        DEFAULT_RATIO
    };
    ratio.clamp(RATIO_MIN, RATIO_MAX)
}

impl Node {
    /// Every pane, left to right and top to bottom.
    pub fn leaves(&self) -> Vec<PaneId> {
        match self {
            Node::Leaf(id) => vec![*id],
            Node::Split { first, second, .. } => {
                let mut all = first.leaves();
                all.extend(second.leaves());
                all
            }
        }
    }

    pub fn contains(&self, id: PaneId) -> bool {
        match self {
            Node::Leaf(leaf) => *leaf == id,
            Node::Split { first, second, .. } => first.contains(id) || second.contains(id),
        }
    }

    /// Splits pane `target` in two along `axis` at an even ratio; `new` goes second (right or
    /// below). False when `target` is not in the tree.
    pub fn split(&mut self, target: PaneId, axis: Axis, new: PaneId) -> bool {
        let side = match axis {
            Axis::Row => Side::Right,
            Axis::Column => Side::Bottom,
        };
        self.insert_leaf(target, side, new)
    }

    /// Replaces leaf `target` with an even split that puts `new` on `side`. False when
    /// `target` is missing or `new` is already in the tree.
    pub fn insert(&mut self, target: PaneId, side: Side, new: PaneId) -> bool {
        if self.contains(new) {
            return false;
        }
        self.insert_leaf(target, side, new)
    }

    fn insert_leaf(&mut self, target: PaneId, side: Side, new: PaneId) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                let (axis, first, second) = match side {
                    Side::Left => (Axis::Row, new, target),
                    Side::Right => (Axis::Row, target, new),
                    Side::Top => (Axis::Column, new, target),
                    Side::Bottom => (Axis::Column, target, new),
                };
                *self = Node::Split {
                    axis,
                    ratio: DEFAULT_RATIO,
                    first: Box::new(Node::Leaf(first)),
                    second: Box::new(Node::Leaf(second)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => {
                first.insert_leaf(target, side, new) || second.insert_leaf(target, side, new)
            }
        }
    }

    /// Removes pane `id`; its sibling takes the parent's place, and any split that remains
    /// keeps its ratio. `None` when `id` was the only pane (the caller closes the tab), and
    /// the tree unchanged when `id` is not in it.
    pub fn remove(self, id: PaneId) -> Option<Node> {
        match self {
            Node::Leaf(leaf) if leaf == id => None,
            leaf @ Node::Leaf(_) => Some(leaf),
            Node::Split {
                axis,
                ratio,
                first,
                second,
            } => match (first.remove(id), second.remove(id)) {
                // Only one side can hold the pane; the other comes back whole, so a side that
                // disappeared leaves its sibling to take over.
                (None, Some(rest)) | (Some(rest), None) => Some(rest),
                (Some(first), Some(second)) => Some(Node::Split {
                    axis,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (None, None) => None,
            },
        }
    }

    /// Removes `pane` (same collapse as [`Self::remove`], so other ratios stay) and inserts it
    /// on `side` of `target`. False, and the tree unchanged, when `pane` is `target`, either
    /// id is missing, or it is the only pane.
    pub fn move_pane(&mut self, pane: PaneId, target: PaneId, side: Side) -> bool {
        if pane == target || !self.contains(pane) || !self.contains(target) {
            return false;
        }
        // Copy first so a remove that cannot be reinserted leaves this tree alone.
        let Some(mut next) = self.clone().remove(pane) else {
            return false;
        };
        if !next.insert(target, side, pane) {
            return false;
        }
        *self = next;
        true
    }

    /// Sets the ratio of the split with `in_first` in its first subtree and `in_second` in its
    /// second, clamped to [`RATIO_MIN`]..=[`RATIO_MAX`]. Non-finite input becomes an even
    /// split. False when no split separates that pair.
    pub fn set_ratio(&mut self, in_first: PaneId, in_second: PaneId, ratio: f32) -> bool {
        match self {
            Node::Leaf(_) => false,
            Node::Split {
                ratio: share,
                first,
                second,
                ..
            } => {
                if first.contains(in_first) && second.contains(in_second) {
                    *share = clamp_ratio(ratio);
                    true
                } else {
                    first.set_ratio(in_first, in_second, ratio)
                        || second.set_ratio(in_first, in_second, ratio)
                }
            }
        }
    }

    /// Where each pane sits when the whole tree fills `area`. Each split gives `ratio` of its
    /// length along the axis to `first`.
    pub fn layout(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        match self {
            Node::Leaf(id) => vec![(*id, area)],
            Node::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (a, b) = match axis {
                    Axis::Row => {
                        let w = area.w * ratio;
                        (
                            Rect { w, ..area },
                            Rect {
                                x: area.x + w,
                                w: area.w - w,
                                ..area
                            },
                        )
                    }
                    Axis::Column => {
                        let h = area.h * ratio;
                        (
                            Rect { h, ..area },
                            Rect {
                                y: area.y + h,
                                h: area.h - h,
                                ..area
                            },
                        )
                    }
                };
                let mut all = first.layout(a);
                all.extend(second.layout(b));
                all
            }
        }
    }

    /// The pane next to `from` in `dir`: the one that touches its edge and shares the most of
    /// it. `None` at the edge of the tab.
    pub fn neighbour(&self, from: PaneId, dir: Direction) -> Option<PaneId> {
        const EPS: f32 = 1e-4;
        let all = self.layout(Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        });
        let (_, me) = all.iter().find(|(id, _)| *id == from)?;
        let overlap = |a0: f32, a1: f32, b0: f32, b1: f32| (a1.min(b1) - a0.max(b0)).max(0.0);
        all.iter()
            .filter(|(id, _)| *id != from)
            .filter_map(|(id, r)| {
                let (touches, shared) = match dir {
                    Direction::Left => (
                        (r.x + r.w - me.x).abs() < EPS,
                        overlap(me.y, me.y + me.h, r.y, r.y + r.h),
                    ),
                    Direction::Right => (
                        (me.x + me.w - r.x).abs() < EPS,
                        overlap(me.y, me.y + me.h, r.y, r.y + r.h),
                    ),
                    Direction::Up => (
                        (r.y + r.h - me.y).abs() < EPS,
                        overlap(me.x, me.x + me.w, r.x, r.x + r.w),
                    ),
                    Direction::Down => (
                        (me.y + me.h - r.y).abs() < EPS,
                        overlap(me.x, me.x + me.w, r.x, r.x + r.w),
                    ),
                };
                (touches && shared > EPS).then_some((*id, shared))
            })
            // The most shared edge wins; on a tie the earlier pane (top or left) does.
            .fold(None::<(PaneId, f32)>, |best, (id, shared)| match best {
                Some((_, s)) if s >= shared - EPS => best,
                _ => Some((id, shared)),
            })
            .map(|(id, _)| id)
    }
}

/// The side of a pane under `(rel_x, rel_y)`, as fractions of that pane. The axis farther
/// from the center wins; a tie, including `(0.5, 0.5)`, is top or bottom, so the center is
/// [`Side::Bottom`]. Non-finite input is [`Side::Bottom`].
pub fn drop_side(rel_x: f32, rel_y: f32) -> Side {
    // A NaN comparison is never true, so it would not follow the rule below.
    if !rel_x.is_finite() || !rel_y.is_finite() {
        return Side::Bottom;
    }
    let prefers_vertical = (rel_x - 0.5).abs() > (rel_y - 0.5).abs();
    if prefers_vertical {
        if rel_x < 0.5 { Side::Left } else { Side::Right }
    } else if rel_y < 0.5 {
        Side::Top
    } else {
        Side::Bottom
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Pane 1 on the left; 2 top right; 3 bottom right.
    fn l_shape() -> Node {
        let mut t = Node::Leaf(1);
        assert!(t.split(1, Axis::Row, 2));
        assert!(t.split(2, Axis::Column, 3));
        t
    }

    fn leaf(id: PaneId) -> Node {
        Node::Leaf(id)
    }

    fn row(ratio: f32, first: Node, second: Node) -> Node {
        Node::Split {
            axis: Axis::Row,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    fn column(ratio: f32, first: Node, second: Node) -> Node {
        Node::Split {
            axis: Axis::Column,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    const UNIT: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };

    fn at(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn splitting_puts_the_new_pane_second_and_keeps_order() {
        let t = l_shape();
        assert_eq!(t.leaves(), [1, 2, 3]);
        assert_eq!(t, row(0.5, leaf(1), column(0.5, leaf(2), leaf(3))));
    }

    #[test]
    fn splitting_a_missing_pane_changes_nothing() {
        let mut t = l_shape();
        assert!(!t.split(9, Axis::Row, 4));
        assert_eq!(t.leaves(), [1, 2, 3]);
    }

    #[test]
    fn closing_a_pane_collapses_its_parent_into_the_sibling() {
        // Closing 3 leaves 2 to fill the right half: the column split is gone.
        let t = l_shape().remove(3).unwrap();
        assert_eq!(t, row(0.5, leaf(1), leaf(2)));
        // Closing 1 leaves the column alone: it takes the whole tab.
        let t = l_shape().remove(1).unwrap();
        assert_eq!(t, column(0.5, leaf(2), leaf(3)));
    }

    #[test]
    fn the_last_pane_cannot_be_removed_and_unknown_ones_change_nothing() {
        assert_eq!(Node::Leaf(1).remove(1), None);
        assert_eq!(l_shape().remove(9), Some(l_shape()));
    }

    #[test]
    fn focus_moves_to_the_pane_that_touches_the_edge() {
        let t = l_shape();
        assert_eq!(
            t.neighbour(1, Direction::Right),
            Some(2),
            "top wins the tie"
        );
        assert_eq!(t.neighbour(2, Direction::Left), Some(1));
        assert_eq!(t.neighbour(3, Direction::Left), Some(1));
        assert_eq!(t.neighbour(2, Direction::Down), Some(3));
        assert_eq!(t.neighbour(3, Direction::Up), Some(2));
    }

    #[test]
    fn there_is_no_neighbour_past_the_edge() {
        let t = l_shape();
        assert_eq!(t.neighbour(1, Direction::Left), None);
        assert_eq!(t.neighbour(1, Direction::Up), None);
        assert_eq!(t.neighbour(2, Direction::Up), None);
        assert_eq!(t.neighbour(3, Direction::Down), None);
        assert_eq!(t.neighbour(3, Direction::Right), None);
        assert_eq!(Node::Leaf(1).neighbour(1, Direction::Right), None);
    }

    #[test]
    fn the_neighbour_sharing_most_of_the_edge_wins() {
        // Pane 1 is tall on the left. On the right 3 and 4 share the top half and 5 has the
        // bottom half. Going right from 1, pane 5 shares half of the edge, 3 and 4 a quarter.
        let right = column(0.5, column(0.5, leaf(3), leaf(4)), leaf(5));
        let mut t = row(0.5, leaf(1), right);
        assert_eq!(t.leaves(), [1, 3, 4, 5]);
        assert_eq!(t.neighbour(1, Direction::Right), Some(5));
        assert_eq!(t.neighbour(3, Direction::Left), Some(1));
        // Cut pane 1 into two stacked halves: from 5 only the lower one touches; from 3 only
        // the upper one does.
        assert!(t.split(1, Axis::Column, 2));
        assert_eq!(t.neighbour(5, Direction::Left), Some(2));
        assert_eq!(t.neighbour(3, Direction::Left), Some(1));
    }

    #[test]
    fn an_even_ratio_cuts_each_split_in_half() {
        assert_eq!(
            l_shape().layout(UNIT),
            vec![
                (1, at(0.0, 0.0, 0.5, 1.0)),
                (2, at(0.5, 0.0, 0.5, 0.5)),
                (3, at(0.5, 0.5, 0.5, 0.5)),
            ]
        );
    }

    #[test]
    fn layout_gives_the_first_child_the_splits_ratio() {
        assert_eq!(
            row(0.3, leaf(1), leaf(2)).layout(UNIT),
            vec![(1, at(0.0, 0.0, 0.3, 1.0)), (2, at(0.3, 0.0, 0.7, 1.0))]
        );
        assert_eq!(
            column(0.3, leaf(1), leaf(2)).layout(UNIT),
            vec![(1, at(0.0, 0.0, 1.0, 0.3)), (2, at(0.0, 0.3, 1.0, 0.7))]
        );
    }

    #[test]
    fn layout_composes_nested_split_ratios() {
        let t = row(0.3, leaf(1), column(0.25, leaf(2), leaf(3)));
        assert_eq!(
            t.layout(UNIT),
            vec![
                (1, at(0.0, 0.0, 0.3, 1.0)),
                (2, at(0.3, 0.0, 0.7, 0.25)),
                (3, at(0.3, 0.25, 0.7, 0.75)),
            ]
        );
    }

    #[test]
    fn set_ratio_clamps_to_the_band_and_replaces_non_finite_with_even() {
        let mut t = row(0.5, leaf(1), leaf(2));
        assert!(t.set_ratio(1, 2, 0.0));
        assert_eq!(t, row(RATIO_MIN, leaf(1), leaf(2)));
        assert!(t.set_ratio(1, 2, 2.0));
        assert_eq!(t, row(RATIO_MAX, leaf(1), leaf(2)));
        assert!(t.set_ratio(1, 2, RATIO_MIN));
        assert_eq!(t, row(RATIO_MIN, leaf(1), leaf(2)));
        assert!(t.set_ratio(1, 2, RATIO_MAX));
        assert_eq!(t, row(RATIO_MAX, leaf(1), leaf(2)));
        assert!(t.set_ratio(1, 2, 0.4));
        assert_eq!(t, row(0.4, leaf(1), leaf(2)));
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(t.set_ratio(1, 2, bad));
            assert_eq!(t, row(0.5, leaf(1), leaf(2)));
        }
    }

    #[test]
    fn set_ratio_does_nothing_when_the_pair_is_not_one_split() {
        let mut t = l_shape();
        let before = t.clone();
        assert!(!t.set_ratio(2, 1, 0.2));
        assert!(!t.set_ratio(3, 2, 0.2));
        assert!(!t.set_ratio(1, 1, 0.2));
        assert!(!t.set_ratio(9, 3, 0.2));
        assert!(!t.set_ratio(1, 9, 0.2));
        assert_eq!(t, before);
    }

    #[test]
    fn set_ratio_changes_the_nested_split_and_keeps_the_ancestor() {
        let mut t = l_shape();
        // Pane 3 is nested in the right half, so the pair still names the root split.
        assert!(t.set_ratio(1, 3, 0.3));
        assert!(t.set_ratio(2, 3, 0.4));
        assert_eq!(t, row(0.3, leaf(1), column(0.4, leaf(2), leaf(3))));
    }

    #[test]
    fn insert_places_the_new_pane_on_the_chosen_side() {
        let mut left = Node::Leaf(1);
        assert!(left.insert(1, Side::Left, 2));
        assert_eq!(left, row(0.5, leaf(2), leaf(1)));

        let mut right = Node::Leaf(1);
        assert!(right.insert(1, Side::Right, 2));
        assert_eq!(right, row(0.5, leaf(1), leaf(2)));

        let mut top = Node::Leaf(1);
        assert!(top.insert(1, Side::Top, 2));
        assert_eq!(top, column(0.5, leaf(2), leaf(1)));

        let mut bottom = Node::Leaf(1);
        assert!(bottom.insert(1, Side::Bottom, 2));
        assert_eq!(bottom, column(0.5, leaf(1), leaf(2)));

        // The new split replaces the target leaf; ratios already on the path stay put.
        let mut t = row(0.3, leaf(1), column(0.4, leaf(2), leaf(3)));
        assert!(t.insert(3, Side::Right, 4));
        assert_eq!(
            t,
            row(
                0.3,
                leaf(1),
                column(0.4, leaf(2), row(0.5, leaf(3), leaf(4)))
            )
        );
    }

    #[test]
    fn insert_on_the_right_or_bottom_matches_split() {
        let mut by_insert = l_shape();
        let mut by_split = l_shape();
        assert!(by_insert.insert(3, Side::Right, 4));
        assert!(by_split.split(3, Axis::Row, 4));
        assert_eq!(by_insert, by_split);

        let mut by_insert = l_shape();
        let mut by_split = l_shape();
        assert!(by_insert.insert(2, Side::Bottom, 4));
        assert!(by_split.split(2, Axis::Column, 4));
        assert_eq!(by_insert, by_split);
    }

    #[test]
    fn insert_does_nothing_for_a_missing_target_or_a_pane_already_present() {
        let mut t = l_shape();
        let before = t.clone();
        assert!(!t.insert(9, Side::Left, 4));
        assert!(!t.insert(1, Side::Right, 2));
        assert!(!t.insert(1, Side::Bottom, 1));
        assert_eq!(t, before);
    }

    #[test]
    fn closing_a_grandchild_keeps_the_ancestors_ratio() {
        let t = row(0.3, leaf(1), column(0.7, leaf(2), leaf(3)));
        assert_eq!(t.clone().remove(9), Some(t.clone()));
        assert_eq!(t.clone().remove(3).unwrap(), row(0.3, leaf(1), leaf(2)));
        assert_eq!(t.clone().remove(2).unwrap(), row(0.3, leaf(1), leaf(3)));
        // The column is the sibling that takes over, so its own ratio comes with it.
        assert_eq!(t.remove(1).unwrap(), column(0.7, leaf(2), leaf(3)));
    }

    #[test]
    fn moving_a_pane_to_the_side_of_its_sibling_reorders_them() {
        let mut to_the_right = row(0.5, leaf(1), leaf(2));
        assert!(to_the_right.move_pane(1, 2, Side::Right));
        assert_eq!(to_the_right.leaves(), [2, 1]);
        assert_eq!(to_the_right, row(0.5, leaf(2), leaf(1)));

        let mut to_the_left = row(0.5, leaf(1), leaf(2));
        assert!(to_the_left.move_pane(2, 1, Side::Left));
        assert_eq!(to_the_left, row(0.5, leaf(2), leaf(1)));
    }

    #[test]
    fn moving_a_pane_above_or_below_its_sibling_changes_the_axis() {
        let mut above = row(0.5, leaf(1), leaf(2));
        assert!(above.move_pane(2, 1, Side::Top));
        assert_eq!(above, column(0.5, leaf(2), leaf(1)));

        let mut below = row(0.5, leaf(1), leaf(2));
        assert!(below.move_pane(2, 1, Side::Bottom));
        assert_eq!(below, column(0.5, leaf(1), leaf(2)));
    }

    #[test]
    fn moving_onto_itself_or_a_missing_pane_or_in_a_one_pane_tree_does_nothing() {
        let mut t = l_shape();
        assert!(t.set_ratio(1, 2, 0.3));
        let before = t.clone();
        assert!(!t.move_pane(2, 2, Side::Left));
        assert!(!t.move_pane(9, 1, Side::Right));
        assert!(!t.move_pane(1, 9, Side::Bottom));
        assert_eq!(t, before);

        let mut only = Node::Leaf(1);
        assert!(!only.move_pane(1, 1, Side::Top));
        assert!(!only.move_pane(1, 2, Side::Right));
        assert_eq!(only, Node::Leaf(1));
    }

    #[test]
    fn moving_into_the_old_sibling_subtree_lands_on_that_pane() {
        // 1 | (2 / 3). Pane 2 sits in 1's sibling, and that column's ratio must survive.
        let mut t = row(0.3, leaf(1), column(0.7, leaf(2), leaf(3)));
        assert!(t.move_pane(1, 2, Side::Right));
        assert_eq!(t.leaves(), [2, 1, 3]);
        assert_eq!(t, column(0.7, row(0.5, leaf(2), leaf(1)), leaf(3)));
    }

    #[test]
    fn moving_a_nested_pane_keeps_the_ancestor_ratio_and_the_same_panes() {
        let mut t = row(
            0.3,
            column(0.4, leaf(1), leaf(2)),
            column(0.6, leaf(3), leaf(4)),
        );
        assert!(t.move_pane(1, 2, Side::Right));
        let mut ids = t.leaves();
        ids.sort_unstable();
        assert_eq!(ids, [1, 2, 3, 4]);
        assert_eq!(
            t,
            row(
                0.3,
                row(0.5, leaf(2), leaf(1)),
                column(0.6, leaf(3), leaf(4))
            )
        );
    }

    #[test]
    fn drop_side_picks_the_quarter_farther_from_the_center() {
        assert_eq!(drop_side(0.1, 0.5), Side::Left);
        assert_eq!(drop_side(0.9, 0.5), Side::Right);
        assert_eq!(drop_side(0.5, 0.1), Side::Top);
        assert_eq!(drop_side(0.5, 0.9), Side::Bottom);
        // Farther from the center on x, even though y is also in the bottom half.
        assert_eq!(drop_side(0.8, 0.6), Side::Right);
        assert_eq!(drop_side(0.4, 0.9), Side::Bottom);
        assert_eq!(drop_side(-0.2, 0.5), Side::Left);
        assert_eq!(drop_side(1.6, 0.5), Side::Right);
        assert_eq!(drop_side(0.5, -0.4), Side::Top);
        assert_eq!(drop_side(0.5, 1.8), Side::Bottom);
    }

    #[test]
    fn drop_side_breaks_an_equal_deviation_toward_top_or_bottom() {
        assert_eq!(drop_side(0.5, 0.5), Side::Bottom);
        assert_eq!(drop_side(0.2, 0.2), Side::Top);
        assert_eq!(drop_side(0.8, 0.8), Side::Bottom);
        assert_eq!(drop_side(0.2, 0.8), Side::Bottom);
        assert_eq!(drop_side(0.8, 0.2), Side::Top);
        assert_eq!(drop_side(0.0, 0.0), Side::Top);
        assert_eq!(drop_side(1.0, 1.0), Side::Bottom);
    }

    #[test]
    fn drop_side_is_bottom_for_a_non_finite_pointer() {
        assert_eq!(drop_side(f32::NAN, 0.1), Side::Bottom);
        assert_eq!(drop_side(0.1, f32::NAN), Side::Bottom);
        assert_eq!(drop_side(f32::INFINITY, 0.0), Side::Bottom);
        assert_eq!(drop_side(0.0, f32::NEG_INFINITY), Side::Bottom);
        assert_eq!(drop_side(f32::NEG_INFINITY, f32::INFINITY), Side::Bottom);
    }
}
