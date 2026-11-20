//! Moving hosts between groups, and the name prompt for a new or renamed group.
//! An empty group is remembered on the host list only: `hosts.json` stores each host's group,
//! and a folder with no host would otherwise disappear while it is being reorganized.

use gpui::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, px,
};

use super::connections_ui::{button, row};
use super::{Shell, ToastKind};
use crate::connections::{self, Connection};
use crate::i18n::tr;
use crate::text_input::{InputColors, TextInput};

pub(super) struct Prompt {
    input: Entity<TextInput>,
    kind: Kind,
}

enum Kind {
    New,
    Rename(String),
    MoveHost { ix: usize },
}

/// Puts host `ix` in `target` (`None` ungroups it). A group that loses its last host is kept
/// in `empty` until something removes it. Returns whether a host changed group.
fn move_host(
    connections: &mut [Connection],
    empty: &mut Vec<String>,
    ix: usize,
    target: Option<&str>,
) -> bool {
    let next = target
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned);
    let previous = {
        let Some(slot) = connections.get_mut(ix) else {
            return false;
        };
        if slot.group == next {
            return false;
        }
        std::mem::replace(&mut slot.group, next.clone())
    };
    if let Some(name) = next.as_deref() {
        empty.retain(|group| group != name);
    }
    if let Some(name) = previous
        && !connections
            .iter()
            .any(|connection| connection.group.as_deref() == Some(name.as_str()))
        && !empty.iter().any(|group| group == &name)
    {
        empty.push(name);
    }
    true
}

/// Rewrites `from` to `to` on hosts and in `empty`. An existing `to` absorbs `from`.
fn rename_group(
    connections: &mut [Connection],
    empty: &mut Vec<String>,
    from: &str,
    to: &str,
) -> Result<(), &'static str> {
    let to = to.trim();
    if to.is_empty() {
        return Err("Name is required.");
    }
    if from == to {
        return Ok(());
    }
    let known = connections
        .iter()
        .any(|connection| connection.group.as_deref() == Some(from))
        || empty.iter().any(|group| group == from);
    if !known {
        return Ok(());
    }
    for connection in connections.iter_mut() {
        if connection.group.as_deref() == Some(from) {
            connection.group = Some(to.to_owned());
        }
    }
    empty.retain(|group| group != from && group != to);
    let filled = connections
        .iter()
        .any(|connection| connection.group.as_deref() == Some(to));
    if !filled {
        empty.push(to.to_owned());
    }
    Ok(())
}

/// Drops the group: hosts in it become ungrouped, and the empty-group entry goes away.
fn clear_group(connections: &mut [Connection], empty: &mut Vec<String>, name: &str) {
    empty.retain(|group| group != name);
    for connection in connections.iter_mut() {
        if connection.group.as_deref() == Some(name) {
            connection.group = None;
        }
    }
}

fn add_empty(
    connections: &[Connection],
    empty: &mut Vec<String>,
    name: &str,
) -> Result<(), &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name is required.");
    }
    let known = connections
        .iter()
        .any(|connection| connection.group.as_deref() == Some(name))
        || empty.iter().any(|group| group == name);
    if !known {
        empty.push(name.to_owned());
    }
    Ok(())
}

impl Shell {
    /// Moves one saved host into `group`, or out of its group when `group` is `None`.
    /// The destination opens if it was collapsed. `hosts.json` is written when a host changes.
    pub(crate) fn move_host_to_group(
        &mut self,
        ix: usize,
        group: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        if ix >= self.connections.len() {
            return;
        }
        let target = group
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        let mut connections = self.connections.clone();
        let mut empty = self.hostlist.empty_groups.clone();
        let changed = move_host(&mut connections, &mut empty, ix, target.as_deref());
        if changed && let Err(error) = self.write_book(connections, empty, cx) {
            self.notify_toast(ToastKind::Critical, error, cx);
            return;
        }
        if let Some(name) = target.as_deref() {
            self.hostlist.collapsed_groups.retain(|group| group != name);
            cx.notify();
        }
    }

    /// Removes the group. Hosts that were in it become ungrouped and `hosts.json` is written.
    pub(crate) fn remove_group(&mut self, name: &str, cx: &mut Context<Self>) {
        let mut connections = self.connections.clone();
        let mut empty = self.hostlist.empty_groups.clone();
        clear_group(&mut connections, &mut empty, name);
        if let Err(error) = self.write_book(connections, empty, cx) {
            self.notify_toast(ToastKind::Critical, error, cx);
            return;
        }
        self.hostlist.collapsed_groups.retain(|group| group != name);
    }

    pub(crate) fn prompt_new_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_group_prompt(Kind::New, "", window, cx);
    }

    pub(crate) fn prompt_rename_group(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_group_prompt(Kind::Rename(name.to_owned()), name, window, cx);
    }

    /// Asks for a name, then moves host `ix` into that group (creating it when it is new).
    pub(crate) fn prompt_move_host_to_new_group(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_group_prompt(Kind::MoveHost { ix }, "", window, cx);
    }

    /// Enter confirms and Escape drops the name field.
    pub(super) fn on_group_prompt_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.group_prompt.is_none() {
            return false;
        }
        match event.keystroke.key.as_str() {
            "enter" => self.commit_group_prompt(window, cx),
            "escape" => self.cancel_group_prompt(window, cx),
            _ => return false,
        }
        true
    }

    fn open_group_prompt(
        &mut self,
        kind: Kind,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let t = self.theme;
        let colors = InputColors {
            text: t.text,
            placeholder: t.faint,
            cursor: t.accent,
            selection: t.accent.opacity(0.35),
        };
        let input = cx.new(|cx| {
            let mut input = TextInput::new(tr("Group name"), false, colors, cx);
            input.set_text(text, cx);
            input.select_everything(cx);
            input
        });
        window.focus(&gpui::Focusable::focus_handle(input.read(cx), cx), cx);
        self.group_prompt = Some(Prompt { input, kind });
        cx.notify();
    }

    fn commit_group_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.group_prompt.take() else {
            return;
        };
        let name = prompt.input.read(cx).text().trim().to_owned();
        if name.is_empty() {
            self.group_prompt = Some(prompt);
            self.notify_toast(ToastKind::Critical, tr("Name is required."), cx);
            return;
        }
        let saved = match &prompt.kind {
            Kind::New => self.add_empty_group(&name, cx),
            Kind::Rename(from) => self.rename_saved_group(from, &name, cx),
            Kind::MoveHost { ix } => self.move_into_new_group(*ix, &name, cx),
        };
        if let Err(error) = saved {
            self.group_prompt = Some(prompt);
            self.notify_toast(ToastKind::Critical, error, cx);
            return;
        }
        self.restore_focus(window, cx);
    }

    fn cancel_group_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.group_prompt.take().is_some() {
            self.restore_focus(window, cx);
        }
    }

    fn add_empty_group(&mut self, name: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let mut empty = self.hostlist.empty_groups.clone();
        add_empty(&self.connections, &mut empty, name).map_err(|error| error.to_owned())?;
        self.write_book(self.connections.clone(), empty, cx)
    }

    fn rename_saved_group(
        &mut self,
        from: &str,
        to: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut connections = self.connections.clone();
        let mut empty = self.hostlist.empty_groups.clone();
        rename_group(&mut connections, &mut empty, from, to).map_err(|error| error.to_owned())?;
        self.write_book(connections, empty, cx)?;
        let next = to.trim();
        if from != next {
            let folded = &mut self.hostlist.collapsed_groups;
            let keep = folded.iter().any(|group| group == from || group == next);
            folded.retain(|group| group != from && group != next);
            if keep {
                folded.push(next.to_owned());
            }
        }
        Ok(())
    }

    fn move_into_new_group(
        &mut self,
        ix: usize,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut connections = self.connections.clone();
        let mut empty = self.hostlist.empty_groups.clone();
        add_empty(&connections, &mut empty, name).map_err(|error| error.to_owned())?;
        move_host(&mut connections, &mut empty, ix, Some(name));
        self.write_book(connections, empty, cx)?;
        let target = name.trim();
        self.hostlist
            .collapsed_groups
            .retain(|group| group != target);
        Ok(())
    }

    /// Writes `hosts.json` when a host's group changed, then rebuilds the sidebar.
    /// A failed write leaves both lists as they were.
    fn write_book(
        &mut self,
        connections: Vec<Connection>,
        empty: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if connections != self.connections {
            if let Some(error) = &self.store_error {
                return Err(error.clone());
            }
            if let Some(dir) = crate::settings::dir() {
                connections::save(&dir, &connections).map_err(|error| error.to_string())?;
            }
            self.connections = connections;
        }
        self.hostlist.empty_groups = empty;
        self.refresh_hosts();
        cx.notify();
        Ok(())
    }

    pub(super) fn render_group_prompt(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let prompt = self.group_prompt.as_ref()?;
        let t = &self.theme;
        let viewport = window.viewport_size();
        let title = match prompt.kind {
            Kind::Rename(_) => tr("Rename group"),
            Kind::New | Kind::MoveHost { .. } => tr("New group"),
        };
        let card = div()
            .id("group-prompt")
            .w(px(360.0_f32.min(f32::from(viewport.width) - 32.0)))
            .flex()
            .flex_col()
            .rounded(px(16.))
            .border_1()
            .border_color(t.border)
            .bg(t.popup)
            .text_color(t.text)
            .on_mouse_down_out(cx.listener(|shell, _, window, cx| {
                shell.cancel_group_prompt(window, cx);
            }))
            .child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(t.hairline)
                    .text_size(px(14.))
                    .child(title),
            )
            .child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .child(row(tr("Group name"), &prompt.input, t)),
            )
            .child(
                div()
                    .px(px(16.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(t.hairline)
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(button("group-cancel", tr("Cancel"), false, t).on_click(
                        cx.listener(|shell, _, window, cx| shell.cancel_group_prompt(window, cx)),
                    ))
                    .child(button("group-save", tr("Save"), true, t).on_click(
                        cx.listener(|shell, _, window, cx| shell.commit_group_prompt(window, cx)),
                    )),
            );
        Some(group_overlay(card.into_any_element(), viewport, t))
    }
}

fn group_overlay(
    card: AnyElement,
    viewport: gpui::Size<gpui::Pixels>,
    t: &crate::theme::Theme,
) -> AnyElement {
    deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .occlude()
                .w(viewport.width)
                .h(viewport.height)
                .bg(t.scrim(0.35))
                .flex()
                .items_center()
                .justify_center()
                .child(card),
        ),
    )
    .priority(3)
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{add_empty, clear_group, move_host, rename_group};
    use crate::connections::Connection;

    fn host(name: &str, group: Option<&str>) -> Connection {
        Connection {
            name: name.into(),
            host: "10.0.0.5".into(),
            group: group.map(str::to_owned),
            ..Connection::default()
        }
    }

    #[test]
    fn moving_a_host_into_a_group_and_back_out_keeps_the_empty_folder() {
        let mut connections = vec![host("a", None), host("b", Some("Zeta"))];
        let mut empty = vec!["alpha".to_owned()];
        assert!(move_host(&mut connections, &mut empty, 0, Some("alpha")));
        assert_eq!(connections[0].group.as_deref(), Some("alpha"));
        assert!(empty.is_empty());
        assert!(move_host(&mut connections, &mut empty, 0, None));
        assert_eq!(connections[0].group, None);
        assert_eq!(empty, vec!["alpha".to_owned()]);
        assert!(!move_host(&mut connections, &mut empty, 0, None));
        assert_eq!(empty, vec!["alpha".to_owned()]);
    }

    #[test]
    fn moving_the_last_host_out_keeps_the_group() {
        let mut connections = vec![host("a", Some("Zeta")), host("b", Some("Zeta"))];
        let mut empty = Vec::new();
        assert!(move_host(&mut connections, &mut empty, 0, None));
        assert!(empty.is_empty(), "Zeta still has a host");
        assert!(move_host(&mut connections, &mut empty, 1, Some("  ")));
        assert_eq!(connections[1].group, None);
        assert_eq!(empty, vec!["Zeta".to_owned()]);
        let mut again = vec![host("a", Some("Zeta"))];
        let mut stale = vec!["Zeta".to_owned()];
        assert!(move_host(&mut again, &mut stale, 0, None));
        assert_eq!(stale, vec!["Zeta".to_owned()]);
        assert!(!move_host(&mut again, &mut stale, 9, Some("alpha")));
    }

    #[test]
    fn renaming_a_group_rewrites_its_hosts_and_empty_name() {
        let mut connections = vec![host("a", Some("Old")), host("b", None)];
        let mut empty = vec!["Spare".to_owned()];
        assert!(rename_group(&mut connections, &mut empty, "Old", "New").is_ok());
        assert_eq!(connections[0].group.as_deref(), Some("New"));
        assert_eq!(empty, vec!["Spare".to_owned()]);
        assert!(rename_group(&mut connections, &mut empty, "Spare", "Kept").is_ok());
        assert_eq!(empty, vec!["Kept".to_owned()]);
        assert!(rename_group(&mut connections, &mut empty, "New", "Kept").is_ok());
        assert_eq!(connections[0].group.as_deref(), Some("Kept"));
        assert!(empty.is_empty(), "Kept now has a host");
        assert_eq!(
            rename_group(&mut connections, &mut empty, "Kept", " "),
            Err("Name is required.")
        );
        assert!(add_empty(&connections, &mut empty, "  ").is_err());
        assert!(add_empty(&connections, &mut empty, "Kept").is_ok());
        assert!(empty.is_empty(), "a live group is not also an empty one");
    }

    #[test]
    fn removing_a_group_drops_its_name_and_ungroups_hosts() {
        let mut connections = vec![host("a", Some("Zeta")), host("b", Some("alpha"))];
        let mut empty = vec!["Zeta".to_owned(), "Spare".to_owned()];
        clear_group(&mut connections, &mut empty, "Zeta");
        assert_eq!(connections[0].group, None);
        assert_eq!(connections[1].group.as_deref(), Some("alpha"));
        assert_eq!(empty, vec!["Spare".to_owned()]);
        clear_group(&mut connections, &mut empty, "Spare");
        assert!(empty.is_empty());
    }
}
