//! Dragging a split divider sets that split's ratio. The grab target is wider than the 1px
//! rule and sits above the terminals; a nested split does not move its ancestor.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, AppContext, Context, DragMoveEvent, Hsla, InteractiveElement, IntoElement,
    MouseButton, MouseUpEvent, ParentElement, StatefulInteractiveElement, Styled, div, px,
    relative,
};

use crate::chrome::DragGhost;
use crate::split::{Axis, Node, PaneId};

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

/// Which split is being dragged. The pane pair is what [`Node::set_ratio`] matches.
#[derive(Clone, Copy, Debug)]
struct SplitResize {
    in_first: PaneId,
    in_second: PaneId,
    axis: Axis,
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
            .child(rule(axis, self.theme.border))
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
        // Copy the payload out before `set_split_ratio` borrows `cx` to mutate the tree.
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
        self.set_split_ratio(
            resize.in_first,
            resize.in_second,
            ratio_from_pointer(along, origin, length, DIVIDER_PX),
            cx,
        );
    }

    fn set_split_ratio(
        &mut self,
        in_first: PaneId,
        in_second: PaneId,
        ratio: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        tab.tree.set_ratio(in_first, in_second, ratio);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
