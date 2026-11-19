//! Dragging a split divider sets that split's ratio. The grab target is wider than the 1px
//! rule and sits above the terminals; a nested split does not move its ancestor.
//!
//! Dragging a single-pane tab over another tab previews the half of the pane under the pointer.
//! Dropping there moves that pane into the tab (see `split_merge`). Leaving the pane, or ending
//! the drag anywhere else, clears the preview.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, DragMoveEvent, Hsla, InteractiveElement, IntoElement,
    MouseButton, MouseUpEvent, ParentElement, StatefulInteractiveElement, Styled, div, px,
    relative,
};

use crate::chrome::DragGhost;
use crate::split::{Axis, Node, PaneId, Side};
use crate::tabs::DraggedTab;

use super::broadcast_ui::TabId;
use super::{Shell, Tab};

/// Visual thickness reserved before the two panes share the rest.
const DIVIDER_PX: f32 = 1.;
/// Grab target centred on the rule. Wide enough to hit; it draws nothing.
const HANDLE_PX: f32 = 6.;

/// `along` is the pointer on the split axis (x for Row, y for Column).
/// `origin` and `length` are that axis of the split container, same units.
/// `divider` is the visual thickness (1px) reserved in the container.
/// Returns `(along - origin) / (length - divider)` so the divider's leading
/// edge follows the pointer. Not clamped. Non-finite input, or a usable
/// length (`length - divider`) that is not > 0, returns 0.5.
pub fn ratio_from_pointer(along: f32, origin: f32, length: f32, divider: f32) -> f32 {
    let usable = length - divider;
    if along.is_finite()
        && origin.is_finite()
        && length.is_finite()
        && divider.is_finite()
        && usable > 0.
    {
        (along - origin) / usable
    } else {
        0.5
    }
}

/// Whether a dragged tab should preview a split on the pane under the pointer.
/// `source_panes` is how many panes the dragged tab has.
/// `same_tab` is true when the pointer is over that tab's own content.
/// `rel_x`, `rel_y` are the pointer in the target pane's unit square.
/// None when the source is not a single pane, when it is the same tab, when
/// the point is outside 0..=1, or when any input is non-finite.
pub fn split_drop_hint(
    source_panes: usize,
    same_tab: bool,
    rel_x: f32,
    rel_y: f32,
) -> Option<crate::split::Side> {
    if source_panes != 1
        || same_tab
        || !rel_x.is_finite()
        || !rel_y.is_finite()
        || !(0.0..=1.0).contains(&rel_x)
        || !(0.0..=1.0).contains(&rel_y)
    {
        return None;
    }
    Some(crate::split::drop_side(rel_x, rel_y))
}

/// The pointer as fractions of a pane. `None` when the pane has no area, a value is
/// non-finite, or the point falls outside 0..=1.
pub(super) fn pane_rel(
    x: f32,
    y: f32,
    origin_x: f32,
    origin_y: f32,
    width: f32,
    height: f32,
) -> Option<(f32, f32)> {
    if !x.is_finite()
        || !y.is_finite()
        || !origin_x.is_finite()
        || !origin_y.is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || width <= 0.
        || height <= 0.
    {
        return None;
    }
    let rel_x = (x - origin_x) / width;
    let rel_y = (y - origin_y) / height;
    if (0.0..=1.0).contains(&rel_x) && (0.0..=1.0).contains(&rel_y) {
        Some((rel_x, rel_y))
    } else {
        None
    }
}

/// Which split is being dragged. The pane pair is what [`Node::set_ratio`] matches.
#[derive(Clone, Copy, Debug)]
struct SplitResize {
    in_first: PaneId,
    in_second: PaneId,
    axis: Axis,
}

/// Which half of a pane a dragged tab or pane would split into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SplitPreview {
    pub(super) tab: TabId,
    pub(super) pane: PaneId,
    pub(super) side: Side,
}

/// Whether `split_preview` came from a tab merge or a pane rearrange. The two drags share
/// the overlay, and one must not keep the other's hint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreviewKind {
    TabMerge,
    PaneRearrange,
}

/// `left`/`top` of `ratio` is the point as if the rule had no thickness. The rule starts
/// `DIVIDER_PX * ratio` sooner, and the handle overhangs it by the same amount on both sides.
fn handle_nudge(ratio: f32) -> f32 {
    -(ratio * DIVIDER_PX + (HANDLE_PX - DIVIDER_PX) / 2.)
}

/// ElementId is a name plus one integer, so both pane ids share that slot.
fn handle_id(in_first: PaneId, in_second: PaneId) -> (&'static str, u64) {
    (
        "split-resize",
        (u64::from(in_first) << 32) | u64::from(in_second),
    )
}

fn side(share: f32, child: AnyElement) -> gpui::Div {
    div()
        .flex_grow(share)
        .flex_shrink(1.)
        .flex_basis(px(0.))
        .min_w(px(0.))
        .min_h(px(0.))
        .child(child)
}

fn rule(axis: Axis, color: Hsla) -> gpui::Div {
    match axis {
        Axis::Row => div().flex_none().w(px(DIVIDER_PX)).h_full().bg(color),
        Axis::Column => div().flex_none().h(px(DIVIDER_PX)).w_full().bg(color),
    }
}

impl Shell {
    /// The two subtrees share the split by `ratio`, with a draggable rule between them.
    pub(super) fn render_split(
        &self,
        tab: &Tab,
        axis: Axis,
        ratio: f32,
        first: &Node,
        second: &Node,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let split = tab.panes.len() > 1;
        let a = self.render_node(tab, first, split, cx);
        let b = self.render_node(tab, second, split, cx);
        let mut el = div()
            .relative()
            .size_full()
            .flex()
            .when(axis == Axis::Column, |el| el.flex_col())
            .child(side(ratio, a))
            // A 1px hairline, light enough to read on the wallpaper without framing the pane.
            .child(rule(axis, self.theme.text.opacity(0.5)))
            .child(side(1.0 - ratio, b));
        let first_leaves = first.leaves();
        let second_leaves = second.leaves();
        if let (Some(&in_first), Some(&in_second)) = (first_leaves.first(), second_leaves.first()) {
            let handle = self.split_handle(axis, ratio, in_first, in_second, cx);
            el = el.child(handle).on_drag_move(cx.listener(
                move |shell, event: &DragMoveEvent<SplitResize>, _, cx| {
                    shell.on_split_drag(event, in_first, in_second, cx);
                },
            ));
        }
        el.into_any_element()
    }

    fn split_handle(
        &self,
        axis: Axis,
        ratio: f32,
        in_first: PaneId,
        in_second: PaneId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let nudge = px(handle_nudge(ratio));
        div()
            .id(handle_id(in_first, in_second))
            .absolute()
            .occlude()
            .when(axis == Axis::Row, |el| {
                el.cursor_col_resize()
                    .top_0()
                    .bottom_0()
                    .w(px(HANDLE_PX))
                    .left(relative(ratio))
                    .ml(nudge)
            })
            .when(axis == Axis::Column, |el| {
                el.cursor_row_resize()
                    .left_0()
                    .right_0()
                    .h(px(HANDLE_PX))
                    .top(relative(ratio))
                    .mt(nudge)
            })
            .on_drag(
                SplitResize {
                    in_first,
                    in_second,
                    axis,
                },
                |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| DragGhost)
                },
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |shell, event: &MouseUpEvent, _, cx| {
                    if event.click_count == 2 {
                        shell.set_split_ratio(in_first, in_second, 0.5, cx);
                    }
                }),
            )
            .into_any_element()
    }

    fn on_split_drag(
        &mut self,
        event: &DragMoveEvent<SplitResize>,
        in_first: PaneId,
        in_second: PaneId,
        cx: &mut Context<Self>,
    ) {
        // Copy the payload out before the tree is borrowed mutably.
        let resize = *event.drag(cx);
        if resize.in_first != in_first || resize.in_second != in_second {
            return;
        }
        let (along, origin, length) = match resize.axis {
            Axis::Row => (
                f32::from(event.event.position.x),
                f32::from(event.bounds.origin.x),
                f32::from(event.bounds.size.width),
            ),
            Axis::Column => (
                f32::from(event.event.position.y),
                f32::from(event.bounds.origin.y),
                f32::from(event.bounds.size.height),
            ),
        };
        self.move_split_ratio(
            resize.in_first,
            resize.in_second,
            ratio_from_pointer(along, origin, length, DIVIDER_PX),
            cx,
        );
    }

    /// A double-click resets the ratio and writes it at once.
    fn set_split_ratio(
        &mut self,
        in_first: PaneId,
        in_second: PaneId,
        ratio: f32,
        cx: &mut Context<Self>,
    ) {
        if self.change_split_ratio(in_first, in_second, ratio, cx) {
            self.split_ratio_dirty = false;
            self.persist_tabs();
        }
    }

    fn move_split_ratio(
        &mut self,
        in_first: PaneId,
        in_second: PaneId,
        ratio: f32,
        cx: &mut Context<Self>,
    ) {
        if self.change_split_ratio(in_first, in_second, ratio, cx) {
            self.split_ratio_dirty = true;
        }
    }

    fn change_split_ratio(
        &mut self,
        in_first: PaneId,
        in_second: PaneId,
        ratio: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };
        let before = tab.tree.clone();
        if !tab.tree.set_ratio(in_first, in_second, ratio) || tab.tree == before {
            return false;
        }
        cx.notify();
        true
    }

    /// Writes a divider move once the drag is over. The handle's mouse-up does not fire
    /// after a drag, because gpui keeps the pointer, so the next frame does it.
    pub(super) fn flush_split_ratio(&mut self, cx: &App) {
        if self.split_ratio_dirty && !cx.has_active_drag() {
            self.split_ratio_dirty = false;
            self.persist_tabs();
        }
    }

    /// Drops a preview after the drag is gone. Move listeners run before gpui clears it.
    pub(super) fn clear_split_preview_without_drag(&mut self, cx: &App) {
        if self.split_preview.is_some() && !cx.has_active_drag() {
            self.split_preview = None;
            self.split_preview_kind = None;
        }
    }

    /// Half-pane wash for the preview aimed at this leaf, while a tab drag is active.
    pub(super) fn split_drop_overlay(
        &self,
        tab: &Tab,
        pane: PaneId,
        cx: &App,
    ) -> Option<AnyElement> {
        if !cx.has_active_drag() {
            return None;
        }
        let preview = self.split_preview?;
        if preview.tab != tab.id || preview.pane != pane {
            return None;
        }
        Some(half_overlay(preview.side, self.theme.accent))
    }

    /// Sets the preview when the pointer is inside this pane, and clears it only when the
    /// pointer has left and the stored preview names this pane. A sibling hears the same
    /// move; it must not wipe a preview it does not own.
    pub(super) fn on_tab_drag_over_pane(
        &mut self,
        event: &DragMoveEvent<DraggedTab>,
        tab: TabId,
        pane: PaneId,
        cx: &mut Context<Self>,
    ) {
        // This listener is `DraggedTab` only. A leftover pane-rearrange hint is not a merge.
        if self.split_preview_kind == Some(PreviewKind::PaneRearrange) {
            self.split_preview = None;
            self.split_preview_kind = None;
            cx.notify();
        }
        let source_ix = event.drag(cx).ix;
        let inside = pane_rel(
            f32::from(event.event.position.x),
            f32::from(event.event.position.y),
            f32::from(event.bounds.origin.x),
            f32::from(event.bounds.origin.y),
            f32::from(event.bounds.size.width),
            f32::from(event.bounds.size.height),
        );
        if let Some((rel_x, rel_y)) = inside {
            let source_panes = self
                .tabs
                .get(source_ix)
                .map_or(0, |source| source.panes.len());
            let same_tab = source_ix == self.active
                || self
                    .tabs
                    .get(source_ix)
                    .is_some_and(|source| source.id == tab);
            let Some(side) = split_drop_hint(source_panes, same_tab, rel_x, rel_y) else {
                return;
            };
            let next = SplitPreview { tab, pane, side };
            if self.split_preview != Some(next)
                || self.split_preview_kind != Some(PreviewKind::TabMerge)
            {
                self.split_preview = Some(next);
                self.split_preview_kind = Some(PreviewKind::TabMerge);
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
}

fn half_overlay(side: Side, accent: Hsla) -> AnyElement {
    let el = div()
        .absolute()
        .bg(accent.opacity(0.25))
        .border_1()
        .border_color(accent);
    match side {
        Side::Left => el.top_0().bottom_0().left_0().w_1_2(),
        Side::Right => el.top_0().bottom_0().right_0().w_1_2(),
        Side::Top => el.left_0().right_0().top_0().h_1_2(),
        Side::Bottom => el.left_0().right_0().bottom_0().h_1_2(),
    }
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_pane_over_another_tab_hints_the_quarter_under_the_pointer() {
        assert_eq!(split_drop_hint(1, false, 0.1, 0.5), Some(Side::Left));
        assert_eq!(split_drop_hint(1, false, 0.9, 0.5), Some(Side::Right));
        assert_eq!(split_drop_hint(1, false, 0.5, 0.1), Some(Side::Top));
        assert_eq!(split_drop_hint(1, false, 0.5, 0.9), Some(Side::Bottom));
        assert_eq!(split_drop_hint(1, false, 0.8, 0.6), Some(Side::Right));
    }

    #[test]
    fn the_edges_of_the_unit_square_still_hint() {
        assert_eq!(split_drop_hint(1, false, 0.0, 0.5), Some(Side::Left));
        assert_eq!(split_drop_hint(1, false, 1.0, 0.5), Some(Side::Right));
        assert_eq!(split_drop_hint(1, false, 0.5, 0.0), Some(Side::Top));
        assert_eq!(split_drop_hint(1, false, 0.5, 1.0), Some(Side::Bottom));
    }

    #[test]
    fn a_drag_from_a_split_tab_does_not_hint() {
        assert_eq!(split_drop_hint(0, false, 0.1, 0.5), None);
        assert_eq!(split_drop_hint(2, false, 0.1, 0.5), None);
        assert_eq!(split_drop_hint(2, false, 0.9, 0.5), None);
        assert_eq!(split_drop_hint(3, false, 0.8, 0.6), None);
    }

    #[test]
    fn dragging_over_the_source_tab_does_not_hint() {
        assert_eq!(split_drop_hint(1, true, 0.1, 0.5), None);
        assert_eq!(split_drop_hint(1, true, 0.5, 0.9), None);
        assert_eq!(split_drop_hint(1, true, 0.8, 0.6), None);
    }

    #[test]
    fn a_pointer_outside_the_pane_does_not_hint() {
        assert_eq!(split_drop_hint(1, false, -0.1, 0.5), None);
        assert_eq!(split_drop_hint(1, false, 1.1, 0.5), None);
        assert_eq!(split_drop_hint(1, false, 0.5, -0.1), None);
        assert_eq!(split_drop_hint(1, false, 0.5, 1.1), None);
    }

    #[test]
    fn a_non_finite_pointer_does_not_hint() {
        assert_eq!(split_drop_hint(1, false, f32::NAN, 0.5), None);
        assert_eq!(split_drop_hint(1, false, 0.1, f32::NAN), None);
        assert_eq!(split_drop_hint(1, false, f32::INFINITY, 0.5), None);
        assert_eq!(split_drop_hint(1, false, 0.5, f32::NEG_INFINITY), None);
        assert_eq!(
            split_drop_hint(1, false, f32::NEG_INFINITY, f32::INFINITY),
            None
        );
    }

    #[test]
    fn a_point_inside_the_pane_is_a_fraction_of_its_bounds() {
        assert_eq!(pane_rel(25., 50., 0., 0., 100., 100.), Some((0.25, 0.5)));
        assert_eq!(pane_rel(10., 20., 10., 20., 40., 80.), Some((0., 0.)));
        assert_eq!(pane_rel(50., 100., 10., 20., 40., 80.), Some((1., 1.)));
    }

    #[test]
    fn a_point_outside_the_pane_bounds_is_not_inside() {
        assert_eq!(pane_rel(9., 20., 10., 20., 40., 80.), None);
        assert_eq!(pane_rel(51., 20., 10., 20., 40., 80.), None);
        assert_eq!(pane_rel(10., 19., 10., 20., 40., 80.), None);
        assert_eq!(pane_rel(10., 101., 10., 20., 40., 80.), None);
    }

    #[test]
    fn a_pane_with_no_area_or_a_non_finite_measure_has_no_point() {
        assert_eq!(pane_rel(1., 1., 0., 0., 0., 10.), None);
        assert_eq!(pane_rel(1., 1., 0., 0., 10., 0.), None);
        assert_eq!(pane_rel(1., 1., 0., 0., -5., 10.), None);
        assert_eq!(pane_rel(f32::NAN, 1., 0., 0., 10., 10.), None);
        assert_eq!(pane_rel(1., 1., f32::INFINITY, 0., 10., 10.), None);
        assert_eq!(pane_rel(1., 1., 0., 0., f32::NAN, 10.), None);
    }

    #[test]
    fn mid_pointer_is_the_fraction_of_the_usable_length() {
        assert_eq!(ratio_from_pointer(40., 10., 101., 1.), 0.3);
    }

    #[test]
    fn the_leading_edge_is_zero_and_the_trailing_usable_edge_is_one() {
        assert_eq!(ratio_from_pointer(10., 10., 101., 1.), 0.);
        assert_eq!(ratio_from_pointer(110., 10., 101., 1.), 1.);
    }

    #[test]
    fn a_pointer_past_either_end_is_left_unclamped() {
        assert_eq!(ratio_from_pointer(0., 10., 101., 1.), -0.1);
        assert_eq!(ratio_from_pointer(210., 10., 101., 1.), 2.);
    }

    #[test]
    fn non_finite_input_or_no_usable_length_is_even() {
        assert_eq!(ratio_from_pointer(f32::NAN, 0., 10., 1.), 0.5);
        assert_eq!(ratio_from_pointer(1., f32::INFINITY, 10., 1.), 0.5);
        assert_eq!(ratio_from_pointer(1., 0., f32::NEG_INFINITY, 1.), 0.5);
        assert_eq!(ratio_from_pointer(1., 0., 10., f32::NAN), 0.5);
        assert_eq!(
            ratio_from_pointer(f32::NEG_INFINITY, f32::NAN, f32::INFINITY, f32::NAN),
            0.5
        );
        assert_eq!(ratio_from_pointer(1., 0., 1., 1.), 0.5);
        assert_eq!(ratio_from_pointer(1., 0., 0.5, 1.), 0.5);
    }

    #[test]
    fn handle_nudge_centres_the_grab_target_on_the_rule() {
        let ratio = 0.3;
        let length = 101.;
        let left = length * ratio + handle_nudge(ratio);
        let centre = left + HANDLE_PX / 2.;
        let rule_centre = (length - DIVIDER_PX) * ratio + DIVIDER_PX / 2.;
        assert_eq!(centre, rule_centre);

        let ratio = 0.5;
        let left = length * ratio + handle_nudge(ratio);
        assert_eq!(left + HANDLE_PX / 2., length * ratio);
    }
}
