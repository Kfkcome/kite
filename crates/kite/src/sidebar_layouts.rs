//! The sidebar's Layouts section, docked under the host list and resized from its top edge.

use crate::hover::HoverFade as _;
use gpui::{
    AppContext as _, Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseUpEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, Transformation,
    div, percentage, px,
};

use super::{FOOTER_HEIGHT, Shell, SidebarState, Theme};
use crate::i18n::{fill, tr};
use crate::icons::{self, icon};
use crate::settings::{LAYOUTS_DEFAULT, LAYOUTS_MAX, LAYOUTS_MIN};

/// Drag payload for the layouts splitter. The resize draws nothing under the pointer.
pub struct LayoutsResize;

/// Grab strip centred on the 1px rule.
const HANDLE_PX: f32 = 8.;
/// Sidebar header, search field and the local-terminal row, matching those elements' heights.
const SIDEBAR_CHROME: f32 = 37. + 32. + 32.;
/// Host list keeps this much when the layouts pane is as tall as the window allows.
const HOSTS_MIN: f32 = 96.;
/// Titlebar plus the chrome above the host list, and the rows the list must keep.
const HOSTS_RESERVE: f32 = crate::theme::TITLEBAR_HEIGHT + SIDEBAR_CHROME + HOSTS_MIN;

pub(super) fn layout_hits(query: &str, layouts: &[crate::layouts::SavedLayout]) -> Vec<usize> {
    let names: Vec<&str> = layouts.iter().map(|layout| layout.name.as_str()).collect();
    let labels: Vec<String> = layouts.iter().map(crate::layouts::search_text).collect();
    crate::picker::rank_named(query, &names, &labels)
}

/// Height of the docked pane for a pointer at window y `pointer_y`:
/// the gap from the pointer down to the footer, capped so the host list stays usable.
pub(crate) fn dragged_layouts_height(pointer_y: f32, window_height: f32) -> f32 {
    if !pointer_y.is_finite() || !window_height.is_finite() {
        return LAYOUTS_DEFAULT;
    }
    layouts_display_height(window_height - FOOTER_HEIGHT - pointer_y, window_height)
}

/// `configured` capped against this window so the host list stays usable.
pub(crate) fn layouts_display_height(configured: f32, window_height: f32) -> f32 {
    let configured = if configured.is_finite() {
        configured
    } else {
        LAYOUTS_DEFAULT
    };
    let room = if window_height.is_finite() {
        (window_height - FOOTER_HEIGHT - HOSTS_RESERVE).max(0.0)
    } else {
        LAYOUTS_MAX
    };
    let max = LAYOUTS_MAX.min(room);
    if max < LAYOUTS_MIN {
        return max;
    }
    configured.clamp(LAYOUTS_MIN, max)
}

pub(super) fn layouts_dock(
    s: &SidebarState<'_>,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::AnyElement {
    let folded = s
        .collapsed_groups
        .iter()
        .any(|group| group == crate::layouts::GROUP);
    if folded {
        return div()
            .flex_none()
            .border_t_1()
            .border_color(t.ink(0.08))
            .pt(px(6.))
            .px(px(8.))
            .child(layouts_header(true, t, cx))
            .into_any_element();
    }
    let hits: Vec<usize> = (0..s.layouts.len()).collect();
    div()
        .id("layouts-dock")
        .flex_none()
        .h(px(s.layouts_height))
        .min_h_0()
        .relative()
        .flex()
        .flex_col()
        .child(resize_handle(t, cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_none()
                        .pt(px(6.))
                        .px(px(8.))
                        .child(layouts_header(false, t, cx)),
                )
                .child(
                    div()
                        .id("layouts-rows")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px(px(8.))
                        .child(layout_rows("layout", &hits, s.layouts, t, cx)),
                ),
        )
        .into_any_element()
}

/// Top-level section label, aligned with `Hosts`, with the collapse chevron on the right.
fn layouts_header(folded: bool, t: &Theme, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    div()
        .id("layouts-header")
        .flex_none()
        .h(px(28.))
        .px(px(8.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .hover_fade(
            "section-layouts-header",
            gpui::transparent_black(),
            t.row_hover,
        )
        .on_click(cx.listener(|shell, _, _, cx| {
            shell.toggle_group(crate::layouts::GROUP, cx);
        }))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(t.text.opacity(crate::theme::by_dpi(0.8, 0.9)))
                .child(tr("Layouts")),
        )
        .child(
            icon(icons::CHEVRON_RIGHT)
                .size(px(12.))
                .text_color(t.muted.opacity(crate::theme::by_dpi(0.7, 1.0)))
                .with_transformation(Transformation::rotate(percentage(if folded {
                    0.0
                } else {
                    0.25
                }))),
        )
}

fn resize_handle(t: &Theme, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    div()
        .id("layouts-resize")
        .group("layouts-resize")
        .absolute()
        .top(px(-((HANDLE_PX - 1.) / 2.)))
        .left_0()
        .right_0()
        .h(px(HANDLE_PX))
        .occlude()
        .cursor_row_resize()
        .flex()
        .flex_col()
        .justify_center()
        .child(
            div()
                .id("layouts-resize-line")
                .h(px(1.))
                .w_full()
                .bg(t.ink(0.08))
                .group_hover("layouts-resize", |style| style.bg(t.ink(0.28))),
        )
        .on_drag(LayoutsResize, |_, _, _, cx| {
            cx.stop_propagation();
            cx.new(|_| crate::chrome::DragGhost)
        })
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|shell, event: &MouseUpEvent, _, cx| {
                let reset = event.click_count == 2;
                shell.update_settings(
                    |s| {
                        if reset {
                            s.sidebar_layouts_height = LAYOUTS_DEFAULT;
                        }
                    },
                    cx,
                );
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|shell, _, _, cx| shell.update_settings(|_| {}, cx)),
        )
}

pub(super) fn layout_rows(
    scope: &'static str,
    hits: &[usize],
    layouts: &[crate::layouts::SavedLayout],
    t: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Div {
    let mut body = div().pt(px(4.)).flex().flex_col().gap(px(2.));
    for &ix in hits {
        let Some(layout) = layouts.get(ix) else {
            continue;
        };
        body = body.child(layout_row(
            scope,
            ix,
            &layout.name,
            layout.pane_count(),
            t,
            cx,
        ));
    }
    body
}

fn layout_row(
    scope: &'static str,
    ix: usize,
    name: &str,
    panes: usize,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let menu_ix = ix;
    div()
        .id((scope, ix))
        .h(px(29.))
        .px(px(10.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .hover_fade(
            format!("layout-{scope}-{ix}"),
            gpui::transparent_black(),
            t.ink(0.11),
        )
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.open_layout_at(ix, window, cx);
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |shell, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                shell.open_layout_menu(menu_ix, event.position, cx);
            }),
        )
        .child(icon(icons::LAYOUT).size(px(14.)).text_color(t.accent))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(13.))
                .text_color(t.text.opacity(crate::theme::by_dpi(0.8, 0.9)))
                .child(SharedString::from(name.to_owned())),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(if crate::theme::low_dpi() { 12. } else { 11. }))
                .text_color(t.muted.opacity(crate::theme::by_dpi(0.6, 0.85)))
                .child(fill("{} panes", &[&panes.to_string()])),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_height_is_the_gap_from_the_pointer_to_the_footer() {
        let pointer_y = 676.0;
        let window_height = 900.0;
        assert_eq!(
            dragged_layouts_height(pointer_y, window_height),
            window_height - FOOTER_HEIGHT - pointer_y
        );
    }

    #[test]
    fn drag_clamps_to_the_settings_range() {
        assert_eq!(dragged_layouts_height(836.0, 900.0), LAYOUTS_MIN);
        assert_eq!(dragged_layouts_height(100.0, 900.0), LAYOUTS_MAX);
    }

    #[test]
    fn a_short_window_caps_the_pane_at_the_room_above_the_host_list() {
        let window_height = 500.0;
        let room = window_height - FOOTER_HEIGHT - HOSTS_RESERVE;
        assert!(room < LAYOUTS_MAX && room > LAYOUTS_MIN, "{room}");
        assert_eq!(layouts_display_height(LAYOUTS_MAX, window_height), room);
    }

    #[test]
    fn a_window_too_short_for_the_floor_shrinks_the_pane() {
        let window_height = 300.0;
        let room = window_height - FOOTER_HEIGHT - HOSTS_RESERVE;
        assert!(room < LAYOUTS_MIN, "{room}");
        assert_eq!(layouts_display_height(LAYOUTS_DEFAULT, window_height), room);
    }

    #[test]
    fn a_non_finite_pointer_keeps_the_default() {
        assert_eq!(dragged_layouts_height(f32::NAN, 900.0), LAYOUTS_DEFAULT);
        assert_eq!(
            dragged_layouts_height(100.0, f32::INFINITY),
            LAYOUTS_DEFAULT
        );
    }
}
