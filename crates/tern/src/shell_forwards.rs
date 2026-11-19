// Adapted from zeron crates/ui/src/popover.rs (popover_card, menu_row) (MIT).
//! "Port forwards…" on a tab: Netcatty-style tunnel manager with Local/Remote/Dynamic tabs,
//! live status, start/stop/batch controls, and optional persistence onto the host's `forwards`.

use gpui::actions;
use gpui::{
    AppContext, Context, Entity, FocusHandle, Focusable, KeyDownEvent, Pixels, Point, SharedString,
    Window,
};
use tern_ssh::Forward;

use super::{Shell, ToastKind};
use crate::connections;
use crate::forward_spec;
use crate::i18n::{fill, tr};
use crate::keymap::{self, Keymap, ShortcutId};
use crate::session::{Session, SessionNote, Status};
use crate::text_input::{InputColors, TextInput};

actions!(tern, [FillPassword]);

/// The popovers and side panels a tab can have open.
#[derive(Default)]
pub(super) struct Panels {
    pub forwards: Option<ForwardsPanel>,
    pub sftp: Option<super::sftp_ui::SftpPanel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ForwardMode {
    Local,
    Remote,
    Dynamic,
}

pub(super) struct TunnelRow {
    pub forward: Forward,
    pub running_id: Option<u64>,
    pub bound_port: Option<u16>,
    pub saved: bool,
}

pub(super) struct ForwardsPanel {
    pub session: Entity<Session>,
    pub position: Point<Pixels>,
    pub mode: ForwardMode,
    pub listen_port: Entity<TextInput>,
    pub dest_host: Entity<TextInput>,
    pub dest_port: Entity<TextInput>,
    pub bind_host: Entity<TextInput>,
    pub save_to_host: bool,
    pub error: Option<String>,
}

fn parse_port(text: &str, allow_zero: bool) -> Result<u16, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(tr("Enter a port number.").to_owned());
    }
    let port = trimmed
        .parse::<u16>()
        .map_err(|_| tr("Invalid port number.").to_owned())?;
    if port == 0 && !allow_zero {
        return Err(tr("Port must be between 1 and 65535.").to_owned());
    }
    Ok(port)
}

pub(super) fn build_forward(
    mode: ForwardMode,
    listen_port: &str,
    dest_host: &str,
    dest_port: &str,
    bind_host: &str,
) -> Result<Forward, String> {
    let bind = if bind_host.trim().is_empty() {
        "127.0.0.1".to_owned()
    } else {
        bind_host.trim().to_owned()
    };
    let listen = parse_port(listen_port, mode == ForwardMode::Remote)?;
    match mode {
        ForwardMode::Dynamic => Ok(Forward::Dynamic {
            bind_host: bind,
            bind_port: listen,
        }),
        ForwardMode::Local => {
            let dest = if dest_host.trim().is_empty() {
                "127.0.0.1".to_owned()
            } else {
                dest_host.trim().to_owned()
            };
            let target_port = parse_port(dest_port, false)?;
            Ok(Forward::Local {
                bind_host: bind,
                bind_port: listen,
                dest_host: dest,
                dest_port: target_port,
            })
        }
        ForwardMode::Remote => {
            let dest = if dest_host.trim().is_empty() {
                "127.0.0.1".to_owned()
            } else {
                dest_host.trim().to_owned()
            };
            let target_port = parse_port(dest_port, false)?;
            Ok(Forward::Remote {
                bind_host: bind,
                bind_port: listen,
                dest_host: dest,
                dest_port: target_port,
            })
        }
    }
}

impl Shell {
    pub(crate) fn open_forwards_panel(
        &mut self,
        ix: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.tabs.get(ix).map(|t| t.session().clone()) else {
            return;
        };
        self.open_forwards_for_session(session, position, window, cx);
    }

    pub(crate) fn open_forwards_for_session(
        &mut self,
        session: Entity<Session>,
        position: Point<Pixels>,
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
        let mut field = |placeholder: &str| {
            let input = cx.new(|cx| TextInput::new(placeholder.to_owned(), false, colors, cx));
            cx.observe(&input, |_, _, cx| cx.notify()).detach();
            input
        };
        let listen_port = field("8080");
        let dest_host = field("127.0.0.1");
        let dest_port = field("5432");
        let bind_host = field("127.0.0.1");
        window.focus(&listen_port.focus_handle(cx), cx);
        let save_to_host = self
            .tab_for_session(&session)
            .and_then(|tab| tab.panes.iter().find(|p| p.session == session))
            .is_some_and(|p| self.connections.iter().any(|c| c.name == p.alias));
        self.panels.forwards = Some(ForwardsPanel {
            session,
            position,
            mode: ForwardMode::Local,
            listen_port,
            dest_host,
            dest_port,
            bind_host,
            save_to_host,
            error: None,
        });
        cx.notify();
    }

    pub(super) fn close_forwards_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panels.forwards.take().is_some() {
            self.restore_focus(window, cx);
        }
    }

    pub(super) fn set_forward_mode(&mut self, mode: ForwardMode, cx: &mut Context<Self>) {
        if let Some(panel) = self.panels.forwards.as_mut() {
            panel.mode = mode;
            panel.error = None;
        }
        cx.notify();
    }

    pub(super) fn tunnel_rows(
        &self,
        session: &Entity<Session>,
        host_alias: &str,
        cx: &gpui::App,
    ) -> Vec<TunnelRow> {
        let active = session.read(cx).active_forwards();
        let saved: Vec<Forward> = self
            .connections
            .iter()
            .find(|c| c.name == host_alias)
            .map(|c| {
                c.forwards
                    .iter()
                    .filter_map(|s| forward_spec::parse(s).ok())
                    .collect()
            })
            .unwrap_or_default();

        let mut rows: Vec<TunnelRow> = active
            .iter()
            .map(|info| TunnelRow {
                forward: info.forward.clone(),
                running_id: Some(info.id),
                bound_port: Some(info.bound_port),
                saved: saved.contains(&info.forward),
            })
            .collect();

        for fwd in saved {
            if !rows.iter().any(|r| r.forward == fwd) {
                rows.push(TunnelRow {
                    forward: fwd,
                    running_id: None,
                    bound_port: None,
                    saved: true,
                });
            }
        }
        rows
    }

    pub(super) fn add_panel_forward(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.panels.forwards.as_mut() else {
            return;
        };
        let forward = match build_forward(
            panel.mode,
            panel.listen_port.read(cx).text(),
            panel.dest_host.read(cx).text(),
            panel.dest_port.read(cx).text(),
            panel.bind_host.read(cx).text(),
        ) {
            Ok(f) => f,
            Err(e) => {
                panel.error = Some(e);
                return cx.notify();
            }
        };
        let session = panel.session.clone();
        let save_to_host = panel.save_to_host;
        let host_alias = self
            .tab_for_session(&session)
            .and_then(|t| t.panes.iter().find(|p| p.session == session))
            .map(|p| p.alias.clone());
        let Some(host_alias) = host_alias else {
            return;
        };
        if session.read(cx).status != Status::Connected {
            if let Some(panel) = self.panels.forwards.as_mut() {
                panel.error = Some(tr("Not connected.").into());
            }
            return cx.notify();
        }
        if let Some(panel) = self.panels.forwards.as_mut() {
            panel.error = None;
            panel
                .listen_port
                .update(cx, |i, cx| i.set_text(SharedString::default(), cx));
            panel
                .dest_port
                .update(cx, |i, cx| i.set_text(SharedString::default(), cx));
        }
        if save_to_host {
            self.save_host_forward(&host_alias, &forward, cx);
        }
        session.update(cx, |s, cx| s.add_forward(forward, cx));
    }

    pub(super) fn start_all_panel_forwards(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.panels.forwards.as_ref() else {
            return;
        };
        let session = panel.session.clone();
        if session.read(cx).status != Status::Connected {
            if let Some(panel) = self.panels.forwards.as_mut() {
                panel.error = Some(tr("Not connected.").into());
            }
            return cx.notify();
        }
        let host_alias = self
            .tab_for_session(&session)
            .and_then(|t| t.panes.iter().find(|p| p.session == session))
            .map(|p| p.alias.clone())
            .unwrap_or_default();
        let to_start: Vec<Forward> = self
            .tunnel_rows(&session, &host_alias, cx)
            .into_iter()
            .filter(|r| r.running_id.is_none())
            .map(|r| r.forward)
            .collect();
        session.update(cx, |s, cx| {
            for fwd in to_start {
                s.add_forward(fwd, cx);
            }
        });
    }

    pub(super) fn stop_all_panel_forwards(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.panels.forwards.as_ref() else {
            return;
        };
        let session = panel.session.clone();
        let ids: Vec<u64> = session
            .read(cx)
            .active_forwards()
            .into_iter()
            .map(|f| f.id)
            .collect();
        session.update(cx, |s, cx| {
            for id in ids {
                s.stop_forward(id, cx);
            }
        });
    }

    pub(super) fn save_host_forward(&mut self, alias: &str, forward: &Forward, cx: &mut Context<Self>) {
        let spec = forward.to_string();
        let mut next = self.connections.clone();
        let Some(conn) = next.iter_mut().find(|c| c.name == alias) else {
            return;
        };
        if conn.forwards.iter().any(|f| forward_spec::parse(f).ok().as_ref() == Some(forward)) {
            return;
        }
        conn.forwards.push(spec);
        if let Some(dir) = crate::settings::dir()
            && let Err(e) = connections::save(&dir, &next)
        {
            return self.notify_toast(ToastKind::Critical, e.to_string(), cx);
        }
        self.connections = next;
        self.refresh_hosts();
        cx.notify();
    }

    pub(super) fn remove_host_forward(
        &mut self,
        alias: &str,
        forward: &Forward,
        cx: &mut Context<Self>,
    ) {
        let mut next = self.connections.clone();
        let Some(conn) = next.iter_mut().find(|c| c.name == alias) else {
            return;
        };
        let before = conn.forwards.len();
        conn.forwards
            .retain(|f| forward_spec::parse(f).ok().as_ref() != Some(forward));
        if conn.forwards.len() == before {
            return;
        }
        if let Some(dir) = crate::settings::dir()
            && let Err(e) = connections::save(&dir, &next)
        {
            return self.notify_toast(ToastKind::Critical, e.to_string(), cx);
        }
        self.connections = next;
        self.refresh_hosts();
        cx.notify();
    }

    fn visible_forward_fields(&self, cx: &gpui::App) -> Vec<FocusHandle> {
        let Some(panel) = self.panels.forwards.as_ref() else {
            return Vec::new();
        };
        let mut fields = vec![&panel.listen_port];
        if panel.mode != ForwardMode::Dynamic {
            fields.push(&panel.dest_host);
            fields.push(&panel.dest_port);
        }
        fields.push(&panel.bind_host);
        fields.into_iter().map(|f| f.focus_handle(cx)).collect()
    }

    pub(super) fn on_forwards_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e.keystroke.key.as_str() {
            "escape" => self.close_forwards_panel(window, cx),
            "enter" => self.add_panel_forward(cx),
            "tab" => {
                let fields = self.visible_forward_fields(cx);
                let at = fields
                    .iter()
                    .position(|f| f.is_focused(window))
                    .unwrap_or(0);
                let delta = if e.keystroke.modifiers.shift { -1 } else { 1 };
                let next = crate::tabs::step(at, fields.len(), delta);
                if let Some(f) = fields.get(next) {
                    window.focus(f, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    pub(super) fn render_panels(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        [
            self.render_forwards_panel(cx),
            self.render_sftp_panel(window, cx),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_local_remote_and_dynamic_forwards_with_defaults() {
        assert_eq!(
            build_forward(ForwardMode::Local, "8080", "", "5432", ""),
            Ok(Forward::Local {
                bind_host: "127.0.0.1".into(),
                bind_port: 8080,
                dest_host: "127.0.0.1".into(),
                dest_port: 5432,
            })
        );
        assert_eq!(
            build_forward(ForwardMode::Remote, "0", "localhost", "3000", "0.0.0.0"),
            Ok(Forward::Remote {
                bind_host: "0.0.0.0".into(),
                bind_port: 0,
                dest_host: "localhost".into(),
                dest_port: 3000,
            })
        );
        assert_eq!(
            build_forward(ForwardMode::Dynamic, "1080", "", "", ""),
            Ok(Forward::Dynamic {
                bind_host: "127.0.0.1".into(),
                bind_port: 1080,
            })
        );
        assert!(build_forward(ForwardMode::Local, "0", "db", "5432", "").is_err());
    }
}

// "Fill password": the shortcut and the toasts sessions raise about it; what it does is in
// `session_fill.rs`.

impl Shell {
    /// The tab that still holds `session`, after a merge or a detach may have moved it.
    pub(super) fn tab_for_session(&self, session: &Entity<Session>) -> Option<&super::Tab> {
        self.tabs
            .iter()
            .find(|tab| tab.panes.iter().any(|pane| pane.session == *session))
    }

    /// Drops an SFTP or port-forward panel whose session is no longer in any tab.
    pub(super) fn close_orphan_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sftp_gone = self
            .panels
            .sftp
            .as_ref()
            .is_some_and(|panel| self.tab_for_session(&panel.session).is_none());
        let forwards_gone = self
            .panels
            .forwards
            .as_ref()
            .is_some_and(|panel| self.tab_for_session(&panel.session).is_none());
        if sftp_gone {
            self.close_sftp(window, cx);
        }
        if forwards_gone {
            self.close_forwards_panel(window, cx);
        }
    }

    /// ⌘\ (by default): fill the password into the focused pane, if it is at a prompt.
    pub(super) fn fill_password(&mut self, cx: &mut Context<Self>) {
        // A form or the Settings page has the keyboard; the pane behind it is not the target.
        if self.settings_page.is_some() || self.form.is_some() {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let session = tab.session().clone();
        session.update(cx, |s, cx| s.fill_password(cx));
    }

    /// What a session reports on its own: a forward that could not start, a password prompt
    /// worth a hint, or a line about a fill.
    pub(super) fn on_session_note(
        &mut self,
        session: &Entity<Session>,
        note: &SessionNote,
        cx: &mut Context<Self>,
    ) {
        match note {
            SessionNote::ForwardFailed { forward, error } => self.notify_toast(
                ToastKind::Critical,
                fill("Port forward {} failed: {}", &[forward, error]),
                cx,
            ),
            SessionNote::PasswordPrompt => {
                let in_front = self.settings_page.is_none()
                    && self
                        .tabs
                        .get(self.active)
                        .is_some_and(|t| t.session() == session);
                let combo = cx.global::<Keymap>().combo(ShortcutId::FillPassword);
                // Unbound, there is no shortcut to name.
                if in_front && !combo.is_empty() {
                    let shown = keymap::badge(combo);
                    let text = fill("{} fills the password", &[&shown]);
                    self.notify_toast(ToastKind::Default, text, cx);
                }
            }
            SessionNote::Message(text) => self.notify_toast(ToastKind::Default, text.clone(), cx),
            SessionNote::Problem(text) => self.notify_toast(ToastKind::Critical, text.clone(), cx),
        }
    }
}
