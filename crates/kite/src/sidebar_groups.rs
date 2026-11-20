//! Drag a host onto a group header, an open group, or back out to the ungrouped list.
//! The ghost matches a dragged tab: a short pill under the pointer.

use crate::hover::Accessible as _;
use crate::hover::HoverFade as _;
use gpui::{
    Context, Div, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Render, SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div,
    px,
};

use super::{Shell, SidebarState, Theme};
use crate::i18n::tr;
use crate::icons::{self, icon};
use crate::theme::CONTROL_RADIUS;

/// The host a drag is carrying. `ix` is its place in `connections`.
#[derive(Clone)]
pub(super) struct DraggedHost {
    pub ix: usize,
    pub name: String,
    background: Hsla,
    color: Hsla,
}

impl Render for DraggedHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(26.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded(px(CONTROL_RADIUS))
            .bg(self.background)
            .text_color(self.color)
            .text_sm()
            .shadow_md()
            .child(crate::tabs::middle_ellipsis(&self.name, 26))
    }
}

/// `Some` only for a saved host row. Search hits (`found`) stay put.
pub(super) fn dragged_host(scope: &str, ix: usize, name: &str, t: &Theme) -> Option<DraggedHost> {
    if scope != "host" {
        return None;
    }
    Some(DraggedHost {
        ix,
        name: name.to_owned(),
        background: t.popup,
        color: t.text,
    })
}

/// Saved hosts, ungrouped first, then each group. The ungrouped container is always there,
/// so a grouped host can be dropped back out even when nothing is ungrouped.
pub(super) fn append_saved_hosts(
    mut list: Stateful<Div>,
    s: &SidebarState<'_>,
    inner: f32,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let mut body = div()
        .id("ungrouped-hosts")
        .pt(px(4.))
        .flex()
        .flex_col()
        .gap(px(2.));
    body = accept_host(body, t.accent, None, cx);
    for &ix in &s.layout.ungrouped {
        body = body.child(placed_row(ix, s, inner, t, cx));
    }
    list = list.child(body);
    for (n, (name, rows)) in s.layout.groups.iter().enumerate() {
        let folded = s.collapsed_groups.contains(name);
        let toggled = name.clone();
        let menu_name = name.clone();
        let header = accept_host(
            super::section_header(
                ("group-header", n),
                SharedString::from(format!("{name} · {}", rows.len())),
                folded,
                true,
                t,
                cx,
                move |shell, cx| shell.toggle_group(&toggled, cx),
            ),
            t.accent,
            Some(name.clone()),
            cx,
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |shell, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                shell.open_group_menu(&menu_name, event.position, cx);
            }),
        );
        list = list.child(header);
        if folded {
            continue;
        }
        let mut rows_el = div()
            .id(("group-rows", n))
            .pt(px(4.))
            .pl(px(12.))
            .flex()
            .flex_col()
            .gap(px(2.));
        rows_el = accept_host(rows_el, t.accent, Some(name.clone()), cx);
        for &ix in rows {
            rows_el = rows_el.child(placed_row(ix, s, inner, t, cx));
        }
        list = list.child(rows_el);
    }
    list
}

fn placed_row(
    ix: usize,
    s: &SidebarState<'_>,
    inner: f32,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let host = &s.hosts[ix];
    let status = s.open.iter().rev().find_map(|tab| {
        tab.hosts
            .iter()
            .rev()
            .find(|(alias, _)| alias == &host.alias)
            .map(|(_, dot)| *dot)
    });
    let active = s.active_alias == Some(host.alias.as_str());
    super::row("host", ix, host, status, active, false, inner, t, cx)
}

fn accept_host<E>(el: E, accent: Hsla, group: Option<String>, cx: &mut Context<Shell>) -> E
where
    E: StatefulInteractiveElement,
{
    el.drag_over::<DraggedHost>(move |style, _, _, _| style.bg(accent.opacity(0.14)))
        .on_drop(cx.listener(move |shell, dragged: &DraggedHost, _, cx| {
            shell.move_host_to_group(dragged.ix, group.as_deref(), cx);
        }))
}

/// The Hosts title. A drop here ungroups; a right-click starts a group or a connection.
pub(super) fn hosts_header(t: &Theme, cx: &mut Context<Shell>) -> impl IntoElement {
    accept_host(
        div()
            .id("hosts-header")
            .px(px(16.))
            .pt(px(8.))
            .pb(px(4.))
            .h(px(37.))
            .flex()
            .items_center()
            .justify_between(),
        t.accent,
        None,
        cx,
    )
    .on_mouse_down(
        MouseButton::Right,
        cx.listener(|shell, event: &MouseDownEvent, _, cx| {
            shell.open_hosts_menu(event.position, cx);
        }),
    )
    .child(
        div()
            .text_size(px(13.))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(t.text.opacity(crate::theme::by_dpi(0.8, 0.9)))
            .child(tr("Hosts")),
    )
    .child(
        super::icon_button("new-connection", icons::PLUS, 24., 16., t)
            .icon_button(tr("New connection"), t)
            .on_click(cx.listener(|shell, _, window, cx| shell.open_form(None, None, window, cx))),
    )
}

/// The local-terminal row and the padding around it. Both take a drop that ungroups.
pub(super) fn local_terminal(t: &Theme, cx: &mut Context<Shell>) -> impl IntoElement {
    accept_host(
        div()
            .id("sidebar-local-terminal-wrap")
            .px(px(8.))
            .pb(px(4.)),
        t.accent,
        None,
        cx,
    )
    .child(
        div()
            .id("sidebar-local-terminal")
            .h(px(28.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .cursor_pointer()
            .text_size(px(13.))
            .text_color(t.text)
            .hover_fade("sidebar-local", gpui::transparent_black(), t.row_hover)
            .drag_over::<DraggedHost>({
                let accent = t.accent;
                move |style, _, _, _| style.bg(accent.opacity(0.14))
            })
            .on_drop(cx.listener(|shell, dragged: &DraggedHost, _, cx| {
                shell.move_host_to_group(dragged.ix, None, cx);
            }))
            .on_click(cx.listener(|shell, _, window, cx| shell.open_local_terminal(window, cx)))
            .child(icon(icons::TERMINAL).size(px(14.)).text_color(t.accent))
            .child(tr("Local Terminal")),
    )
}

/// Groups from `connections`, plus any `empty_groups` that no host uses yet.
/// Names sort case-insensitively; hosts keep their saved order inside a group.
pub fn layout_with_empty(
    connections: &[crate::connections::Connection],
    empty_groups: &[String],
) -> super::Layout {
    let mut ungrouped = Vec::new();
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (ix, connection) in connections.iter().enumerate() {
        match connection.group.as_deref() {
            None => ungrouped.push(ix),
            Some(name) => match groups.iter_mut().find(|(group, _)| group == name) {
                Some((_, rows)) => rows.push(ix),
                None => groups.push((name.to_owned(), vec![ix])),
            },
        }
    }
    for name in empty_groups {
        let trimmed = name.trim();
        if trimmed.is_empty() || groups.iter().any(|(group, _)| group == trimmed) {
            continue;
        }
        groups.push((trimmed.to_owned(), Vec::new()));
    }
    groups.sort_by_key(|(name, _)| name.to_lowercase());
    super::Layout { ungrouped, groups }
}

#[cfg(test)]
mod tests {
    use super::layout_with_empty;
    use crate::connections::Connection;

    fn conn(name: &str, group: Option<&str>) -> Connection {
        Connection {
            name: name.into(),
            host: "10.0.0.5".into(),
            port: 22,
            user: "deploy".into(),
            group: group.map(str::to_owned),
            ..Connection::default()
        }
    }

    #[test]
    fn empty_groups_join_sorted_folders_without_duplicating_a_live_one() {
        let list = [conn("a", Some("Zeta")), conn("b", None)];
        let empty = [
            "alpha".to_owned(),
            "Zeta".to_owned(),
            "alpha".to_owned(),
            "  ".to_owned(),
        ];
        let laid = layout_with_empty(&list, &empty);
        assert_eq!(laid.ungrouped, vec![1]);
        assert_eq!(
            laid.groups,
            vec![("alpha".to_owned(), vec![]), ("Zeta".to_owned(), vec![0]),]
        );
        // `layout` stays the connections-only view.
        assert_eq!(
            super::super::layout(&list).groups,
            vec![("Zeta".to_owned(), vec![0])]
        );
    }

    #[test]
    fn empty_names_sort_with_live_groups_and_keep_distinct_spellings() {
        let list = [conn("a", Some("Alpha"))];
        let laid = layout_with_empty(&list, &["beta".to_owned(), "alpha".to_owned()]);
        assert_eq!(laid.groups[0].0, "Alpha");
        assert_eq!(laid.groups[0].1, vec![0]);
        assert_eq!(laid.groups[1].0, "alpha");
        assert!(laid.groups[1].1.is_empty());
        assert_eq!(laid.groups[2].0, "beta");
        assert!(laid.groups[2].1.is_empty());
    }
}
