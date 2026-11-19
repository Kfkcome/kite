//! Netcatty-style GUI Tunnel Manager view for `ForwardsPanel`.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Entity, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, anchored, deferred, div, px,
};
use kite_ssh::Forward;

use super::connections_ui::button;
use super::forwards_ui::{ForwardMode, ForwardsPanel, TunnelRow};
use super::Shell;
use crate::i18n::{fill, tr};
use crate::settings_widgets::toggle;
use crate::text_input::TextInput;

impl Shell {
    pub(super) fn render_forwards_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panel = self.panels.forwards.as_ref()?;
        let t = self.theme;
        let session = panel.session.clone();
        let tab = self.tab_for_session(&session)?;
        let label = tab.strip_label();
        let host_alias = tab
            .panes
            .iter()
            .find(|p| p.session == session)
            .map(|p| p.alias.clone())
            .unwrap_or_default();
        let is_saved_host = self.connections.iter().any(|c| c.name == host_alias);
        let rows = self.tunnel_rows(&session, &host_alias, cx);
        let any_running = rows.iter().any(|r| r.running_id.is_some());
        let any_stopped = rows.iter().any(|r| r.running_id.is_none());

        let header = div()
            .px(px(10.))
            .py(px(6.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(t.muted)
                    .child(SharedString::from(fill("Port forwards on {}", &[&label]))),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .when(any_stopped, |el| {
                        el.child(
                            button("forward-start-all", tr("Start all"), false, &t).on_click(
                                cx.listener(|s, _, _, cx| s.start_all_panel_forwards(cx)),
                            ),
                        )
                    })
                    .when(any_running, |el| {
                        el.child(
                            button("forward-stop-all", tr("Stop all"), false, &t)
                                .on_click(cx.listener(|s, _, _, cx| s.stop_all_panel_forwards(cx))),
                        )
                    }),
            );

        let mut list = div().flex().flex_col().gap(px(3.)).max_h(px(190.)).id("forwards-list").overflow_y_scroll();
        if rows.is_empty() {
            list = list.child(
                div()
                    .px(px(10.))
                    .py(px(10.))
                    .text_xs()
                    .text_color(t.muted)
                    .child(tr("No port forwards configured yet")),
            );
        } else {
            for (ix, row) in rows.iter().enumerate() {
                list = list.child(self.render_tunnel_row(ix, row, &session, &host_alias, is_saved_host, cx));
            }
        }

        let mode_tabs = div()
            .flex()
            .gap(px(6.))
            .child(
                button(
                    "fwd-mode-local",
                    tr("Local (-L)"),
                    panel.mode == ForwardMode::Local,
                    &t,
                )
                .on_click(cx.listener(|s, _, _, cx| s.set_forward_mode(ForwardMode::Local, cx))),
            )
            .child(
                button(
                    "fwd-mode-remote",
                    tr("Remote (-R)"),
                    panel.mode == ForwardMode::Remote,
                    &t,
                )
                .on_click(cx.listener(|s, _, _, cx| s.set_forward_mode(ForwardMode::Remote, cx))),
            )
            .child(
                button(
                    "fwd-mode-dynamic",
                    tr("Dynamic (-D)"),
                    panel.mode == ForwardMode::Dynamic,
                    &t,
                )
                .on_click(cx.listener(|s, _, _, cx| s.set_forward_mode(ForwardMode::Dynamic, cx))),
            );

        let form = self.render_forward_form(panel, is_saved_host, cx);

        let mut card = div()
            .id("forwards-panel")
            .occlude()
            .w(px(460.))
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(px(14.))
            .border_1()
            .border_color(t.border)
            .bg(t.popup)
            .shadow_lg()
            .text_size(px(13.))
            .text_color(t.text)
            .on_key_down(cx.listener(Self::on_forwards_key))
            .on_mouse_down_out(cx.listener(|s, _, w, cx| s.close_forwards_panel(w, cx)))
            .child(header)
            .child(list)
            .child(div().h(px(1.)).bg(t.hairline))
            .child(mode_tabs)
            .child(form);

        if let Some(e) = &panel.error {
            card = card.child(
                div()
                    .px(px(6.))
                    .text_size(px(11.))
                    .text_color(t.danger)
                    .child(SharedString::from(e.clone())),
            );
        }

        Some(
            deferred(
                anchored()
                    .position(panel.position)
                    .snap_to_window_with_margin(px(8.))
                    .child(card),
            )
            .priority(2)
            .into_any_element(),
        )
    }

    fn render_tunnel_row(
        &self,
        ix: usize,
        row: &TunnelRow,
        session: &Entity<crate::session::Session>,
        host_alias: &str,
        is_saved_host: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t = self.theme;
        let running = row.running_id.is_some();
        let (kind_label, kind_color, desc) = describe_tunnel(&row.forward, row.bound_port);
        let session_btn = session.clone();
        let fwd_start = row.forward.clone();
        let fwd_save = row.forward.clone();
        let fwd_auto = row.forward.clone();
        let fwd_remove = row.forward.clone();
        let alias_save = host_alias.to_owned();
        let alias_auto = host_alias.to_owned();
        let alias_remove = host_alias.to_owned();
        let running_id = row.running_id;
        let saved = row.saved;
        let auto_start = row.auto_start;

        div()
            .id(("forward-row", ix))
            .px(px(8.))
            .py(px(6.))
            .rounded(px(8.))
            .bg(t.row_hover)
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_none()
                    .size(px(7.))
                    .rounded_full()
                    .bg(if running { t.accent } else { t.faint }),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(6.))
                    .py(px(1.))
                    .rounded(px(5.))
                    .bg(kind_color.opacity(0.16))
                    .text_size(px(10.))
                    .text_color(kind_color)
                    .child(kind_label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .child(SharedString::from(desc)),
            )
            .when(is_saved_host && !saved, |el| {
                el.child(
                    div()
                        .id(("forward-save", ix))
                        .flex_none()
                        .px(px(7.))
                        .py(px(2.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_xs()
                        .text_color(t.accent)
                        .hover(|s| s.bg(t.row_active))
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.save_host_forward(&alias_save, &fwd_save, true, cx);
                        }))
                        .child(tr("Save")),
                )
            })
            .when(saved, |el| {
                el.child(
                    div()
                        .id(("forward-auto", ix))
                        .flex_none()
                        .px(px(7.))
                        .py(px(2.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_xs()
                        .when(auto_start, |el| {
                            el.bg(t.accent.opacity(0.16)).text_color(t.accent)
                        })
                        .when(!auto_start, |el| el.text_color(t.muted))
                        .hover(|s| s.bg(t.row_active))
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.toggle_host_forward_auto(&alias_auto, &fwd_auto, cx);
                        }))
                        .child(tr("Auto")),
                )
            })
            .child(
                div()
                    .id(("forward-toggle", ix))
                    .flex_none()
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_xs()
                    .text_color(if running { t.danger } else { t.accent })
                    .hover(|s| s.bg(t.row_active))
                    .on_click(move |_, _, cx| {
                        session_btn.update(cx, |s, cx| {
                            if let Some(id) = running_id {
                                s.stop_forward(id, cx);
                            } else {
                                s.add_forward(fwd_start.clone(), cx);
                            }
                        });
                    })
                    .child(if running { tr("Stop") } else { tr("Start") }),
            )
            .when(saved, |el| {
                el.child(
                    div()
                        .id(("forward-remove", ix))
                        .flex_none()
                        .size(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .cursor_pointer()
                        .text_xs()
                        .text_color(t.muted)
                        .hover(|s| s.bg(t.row_active))
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.remove_host_forward(&alias_remove, &fwd_remove, cx);
                        }))
                        .child("×"),
                )
            })
    }

    fn render_forward_form(
        &self,
        panel: &ForwardsPanel,
        is_saved_host: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t = self.theme;
        let field_box = |label: &'static str, input: &Entity<TextInput>| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(div().text_xs().text_color(t.muted).child(tr(label)))
                .child(
                    div()
                        .h(px(28.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .rounded(px(7.))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.row_hover)
                        .overflow_hidden()
                        .text_xs()
                        .child(input.clone()),
                )
        };

        let fields = match panel.mode {
            ForwardMode::Local => div()
                .flex()
                .gap(px(6.))
                .child(field_box("Local port", &panel.listen_port))
                .child(field_box("Target host", &panel.dest_host))
                .child(field_box("Target port", &panel.dest_port))
                .child(field_box("Bind address", &panel.bind_host)),
            ForwardMode::Remote => div()
                .flex()
                .gap(px(6.))
                .child(field_box("Remote port", &panel.listen_port))
                .child(field_box("Local host", &panel.dest_host))
                .child(field_box("Local port", &panel.dest_port))
                .child(field_box("Bind address", &panel.bind_host)),
            ForwardMode::Dynamic => div()
                .flex()
                .gap(px(6.))
                .child(field_box("Local port", &panel.listen_port))
                .child(field_box("Bind address", &panel.bind_host)),
        };

        let save_control = if is_saved_host {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .id("forward-save-host")
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .cursor_pointer()
                        .on_click(cx.listener(|s, _, _, cx| {
                            if let Some(p) = s.panels.forwards.as_mut() {
                                p.save_to_host = !p.save_to_host;
                            }
                            cx.notify();
                        }))
                        .child(toggle(&t, panel.save_to_host, "fwd-save-toggle"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(t.muted)
                                .child(tr("Save to host")),
                        ),
                )
                .when(panel.save_to_host, |el| {
                    el.child(
                        div()
                            .id("forward-auto-new")
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .cursor_pointer()
                            .on_click(cx.listener(|s, _, _, cx| {
                                if let Some(p) = s.panels.forwards.as_mut() {
                                    p.auto_start_new = !p.auto_start_new;
                                }
                                cx.notify();
                            }))
                            .child(toggle(&t, panel.auto_start_new, "fwd-auto-toggle"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(t.muted)
                                    .child(tr("Auto-start")),
                            ),
                    )
                })
                .into_any_element()
        } else {
            div()
                .text_xs()
                .text_color(t.faint)
                .child(tr("Session only"))
                .into_any_element()
        };

        let footer = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(save_control)
            .child(
                button("forward-add", tr("Start"), true, &t)
                    .on_click(cx.listener(|s, _, _, cx| s.add_panel_forward(cx))),
            );

        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(fields)
            .child(footer)
    }
}

fn describe_tunnel(
    forward: &Forward,
    bound_port: Option<u16>,
) -> (&'static str, gpui::Hsla, String) {
    let local_color = gpui::hsla(210.0 / 360.0, 0.75, 0.62, 1.0);
    let remote_color = gpui::hsla(32.0 / 360.0, 0.85, 0.60, 1.0);
    let socks_color = gpui::hsla(150.0 / 360.0, 0.65, 0.55, 1.0);
    let fmt_host = |h: &str| {
        if h.contains(':') && !h.starts_with('[') {
            format!("[{h}]")
        } else {
            h.to_owned()
        }
    };
    match forward {
        Forward::Local {
            bind_host,
            bind_port,
            dest_host,
            dest_port,
        } => {
            let port = bound_port.unwrap_or(*bind_port);
            (
                "LOCAL",
                local_color,
                format!("{bind_host}:{port} → {}:{dest_port}", fmt_host(dest_host)),
            )
        }
        Forward::Remote {
            bind_host,
            bind_port,
            dest_host,
            dest_port,
        } => {
            let port = bound_port.unwrap_or(*bind_port);
            (
                "REMOTE",
                remote_color,
                format!("{bind_host}:{port} → {}:{dest_port}", fmt_host(dest_host)),
            )
        }
        Forward::Dynamic {
            bind_host,
            bind_port,
        } => {
            let port = bound_port.unwrap_or(*bind_port);
            ("SOCKS5", socks_color, format!("{bind_host}:{port}"))
        }
    }
}
