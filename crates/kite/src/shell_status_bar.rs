//! Isolated status bar view under the active terminal panel.
//!
//! Owns the 1-second session clock so ticking the elapsed timer or latency
//! only repaints this 24 px strip rather than rebuilding the whole [`Shell`].

use std::time::Duration;

use gpui::{
    Context, Entity, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Task, WeakEntity, Window, div, px,
};

use super::Shell;
use crate::hover::Accessible as _;
use crate::i18n::tr;
use crate::indicator::Dot;
use crate::session::{Session, Status};
use crate::statusline;
use crate::theme::Theme;

type TickKey = (Option<u64>, Option<u128>, bool);

pub(super) struct StatusBar {
    shell: WeakEntity<Shell>,
    theme: Theme,
    bg: Hsla,
    session: Option<Entity<Session>>,
    sftp_open: bool,
    forwards_count: usize,
    visible: bool,
    last_tick: Option<TickKey>,
    _clock: Task<()>,
}

impl StatusBar {
    pub(super) fn new(shell: WeakEntity<Shell>, cx: &mut Context<Self>) -> Self {
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(1))
                    .await;
                let alive = this.update(cx, |bar, cx| bar.tick(cx));
                if alive.is_err() {
                    break;
                }
            }
        });
        Self {
            shell,
            theme: Theme::zeron_dark(),
            bg: Theme::zeron_dark().terminal_background,
            session: None,
            sftp_open: false,
            forwards_count: 0,
            visible: false,
            last_tick: None,
            _clock: clock,
        }
    }

    pub(super) fn sync(
        &mut self,
        theme: Theme,
        bg: Hsla,
        session: Option<Entity<Session>>,
        sftp_open: bool,
        forwards_count: usize,
        visible: bool,
    ) {
        self.theme = theme;
        self.bg = bg;
        self.session = session;
        self.sftp_open = sftp_open;
        self.forwards_count = forwards_count;
        self.visible = visible;
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        if !self.visible {
            self.last_tick = None;
            return;
        }
        let Some(session) = self.session.as_ref() else {
            self.last_tick = None;
            return;
        };
        let s = session.read(cx);
        let connected = s.status == Status::Connected;
        if !connected {
            if self.last_tick.take().is_some() {
                cx.notify();
            }
            return;
        }
        let elapsed_bucket = s.elapsed().map(|d| {
            let secs = d.as_secs();
            if secs < 3600 { secs } else { 3600 + secs / 60 }
        });
        let latency_ms = s.latency().map(|d| (d.as_micros() + 500) / 1000);
        let key = (elapsed_bucket, latency_ms, true);
        if self.last_tick != Some(key) {
            self.last_tick = Some(key);
            cx.notify();
        }
    }
}

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(session) = self.session.clone() else {
            return div().into_any_element();
        };
        let t = &self.theme;
        let (label, dot, latency, elapsed) = {
            let s = session.read(cx);
            (
                s.label(),
                Dot::of(&s.status, s.is_asking()),
                s.latency(),
                s.elapsed(),
            )
        };
        let icon_btn = |id: &'static str, tip: &'static str| {
            div()
                .id(id)
                .size(px(18.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .cursor_pointer()
                .icon_button(tr(tip), t)
        };
        let sftp_shell = self.shell.clone();
        let sftp_session = session.clone();
        let sftp_open = self.sftp_open;
        let sftp_btn = icon_btn("statusline-sftp", "Browse files")
            .on_click(move |_, window, cx| {
                let _ = sftp_shell.update(cx, |shell, cx| {
                    if shell.panels.sftp.is_some() {
                        shell.close_sftp(window, cx);
                    } else {
                        shell.open_sftp_for_session(sftp_session.clone(), window, cx);
                    }
                });
            })
            .child(
                crate::icons::icon(crate::icons::FOLDER)
                    .size(px(12.))
                    .text_color(if sftp_open { t.accent } else { t.muted }),
            );
        let fwd_shell = self.shell.clone();
        let fwd_session = session.clone();
        let forwards_count = self.forwards_count;
        let fwd_btn = icon_btn("statusline-forwards", "Port forwards")
            .on_click(move |e: &gpui::ClickEvent, window, cx| {
                let _ = fwd_shell.update(cx, |shell, cx| {
                    if shell.panels.forwards.is_some() {
                        shell.close_forwards_panel(window, cx);
                    } else {
                        shell.open_forwards_for_session(
                            fwd_session.clone(),
                            e.position(),
                            window,
                            cx,
                        );
                    }
                });
            })
            .child(
                crate::icons::icon(crate::icons::GLOBE)
                    .size(px(12.))
                    .text_color(if forwards_count > 0 {
                        t.accent
                    } else {
                        t.muted
                    }),
            );
        let rec_shell = self.shell.clone();
        let reconnect_btn = icon_btn("statusline-reconnect", "Reconnect current session")
            .on_click(move |_, window, cx| {
                session.update(cx, |s, cx| s.reconnect(cx));
                let _ = rec_shell.update(cx, |shell, cx| shell.restore_focus(window, cx));
            })
            .child(
                crate::icons::icon(crate::icons::RESTART)
                    .size(px(12.))
                    .text_color(t.muted),
            );
        statusline::render(t, &label, dot, latency, elapsed, self.bg)
            .child(sftp_btn)
            .child(fwd_btn)
            .child(reconnect_btn)
            .into_any_element()
    }
}
