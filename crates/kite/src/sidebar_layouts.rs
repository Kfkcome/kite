//! The sidebar's Layouts section: one row per saved layout, with its pane count.

use crate::hover::HoverFade as _;
use gpui::{
    Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};

use super::{Shell, SidebarState, Theme, section_header};
use crate::i18n::{fill, tr};
use crate::icons::{self, icon};

pub(super) fn layout_hits(query: &str, layouts: &[crate::layouts::SavedLayout]) -> Vec<usize> {
    let names: Vec<&str> = layouts.iter().map(|layout| layout.name.as_str()).collect();
    let labels: Vec<String> = layouts.iter().map(crate::layouts::search_text).collect();
    crate::picker::rank_named(query, &names, &labels)
}

pub(super) fn layouts_section(
    s: &SidebarState<'_>,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let folded = s
        .collapsed_groups
        .iter()
        .any(|group| group == crate::layouts::GROUP);
    let label = SharedString::from(format!("{} · {}", tr("Layouts"), s.layouts.len()));
    let mut block = div()
        .flex()
        .flex_col()
        .child(div().mt(px(10.)).h(px(1.)).bg(t.ink(0.08)))
        .child(section_header(
            "layouts-header",
            label,
            folded,
            false,
            t,
            cx,
            |shell, cx| shell.toggle_group(crate::layouts::GROUP, cx),
        ));
    if !folded {
        let hits: Vec<usize> = (0..s.layouts.len()).collect();
        // Same inset grouped host rows use, so the icon sits under the chevron.
        block = block.child(layout_rows("layout", &hits, s.layouts, t, cx).pl(px(12.)));
    }
    block
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
        .px(px(8.))
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
                .text_size(px(11.))
                .text_color(t.muted)
                .child(fill("{} panes", &[&panes.to_string()])),
        )
}
