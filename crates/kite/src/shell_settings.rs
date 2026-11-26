// Adapted from zeron crates/ui/src/shell.rs (settings route: nav column, section list,
// toggle_settings, Escape to close) (MIT).
//! The Settings page. ⌘, swaps the window body for it, as zeron does; Escape or Back returns.
//! Every control writes `settings.json` at once.

use crate::hover::Accessible as _;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};

#[path = "shell_settings_nav.rs"]
mod nav;

use super::Shell;
use crate::i18n::{fill, tr};
use crate::keymap::{Keymap, Record, ShortcutId, badge, record};
use crate::settings::{AppearanceMode, FONT_DEFAULT, FONT_MAX, FONT_MIN, Language};
use crate::settings_widgets as w;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Section {
    Appearance,
    Terminal,
    Shortcuts,
    Connections,
    Snippets,
    Vault,
    Sync,
    About,
}

impl Section {
    const ALL: [Section; 8] = [
        Section::Appearance,
        Section::Terminal,
        Section::Shortcuts,
        Section::Connections,
        Section::Snippets,
        Section::Vault,
        Section::Sync,
        Section::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::Appearance => tr("Appearance"),
            Section::Terminal => tr("Terminal"),
            Section::Shortcuts => tr("Shortcuts"),
            Section::Connections => tr("Connections"),
            Section::Snippets => tr("Snippets"),
            Section::Vault => tr("Vault"),
            Section::Sync => tr("Sync"),
            Section::About => tr("About"),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Appearance => crate::icons::PALETTE,
            Section::Terminal => crate::icons::TERMINAL,
            Section::Shortcuts => crate::icons::KEYBOARD,
            Section::Connections => crate::icons::SERVER,
            Section::Snippets => crate::icons::FILE_CODE,
            Section::Vault => crate::icons::KEY,
            Section::Sync => crate::icons::CLOUD,
            Section::About => crate::icons::INFO,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Section::Appearance => "nav-appearance",
            Section::Terminal => "nav-terminal",
            Section::Shortcuts => "nav-shortcuts",
            Section::Connections => "nav-connections",
            Section::Snippets => "nav-snippets",
            Section::Vault => "nav-vault",
            Section::Sync => "nav-sync",
            Section::About => "nav-about",
        }
    }
}

impl Shell {
    pub(crate) fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_page.take().is_some() {
            self.restore_focus(window, cx);
        } else {
            self.picker = None;
            self.settings_page = Some(Section::Appearance);
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    pub(crate) fn open_settings_at_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = None;
        self.settings_page = Some(Section::Sync);
        self.ensure_sync_ui(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Opens `section` and starts whatever it shows that loads lazily.
    fn select_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.settings_page = Some(section);
        if section == Section::Sync {
            self.ensure_sync_ui(cx);
        }
        if section == Section::Vault {
            self.ensure_vault_ui(cx);
            self.unlock_from_keychain(cx);
        }
        cx.notify();
    }

    pub(crate) fn render_settings(
        &self,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let nav = nav::render(self, section, window, cx);
        let page = match section {
            Section::Appearance => self.appearance_page(cx),
            Section::Terminal => self.terminal_page(cx),
            Section::Shortcuts => self.shortcuts_page(cx),
            Section::Connections => self.connections_page(cx),
            Section::Snippets => self.snippets_page(cx),
            Section::Vault => self.vault_page(cx),
            Section::Sync => self.sync_page(cx),
            Section::About => self.about_page(cx),
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(self.side_tile(
                super::look::snap(self.settings.sidebar_width, self.frame.scale),
                nav,
            ))
            // The page is the body's main tile, so it carries the same tint as every other
            // view of it over a wallpaper.
            .child(
                self.main_tile(self.panel_background()).child(
                    div()
                        .id("settings-scroll")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .overflow_y_scroll()
                        .child(page),
                ),
            )
            .into_any_element()
    }

    fn appearance_page(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = self.theme;
        let size = self.settings.terminal_font_size;
        let (minus, value, plus) = w::stepper(&t, "font-size", format!("{size:.0} pt"));
        let font = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                minus.on_click(
                    cx.listener(|s, _, _, cx| s.change_font(|st| st.step_font(-1.0), cx)),
                ),
            )
            .child(value)
            .child(
                plus.on_click(cx.listener(|s, _, _, cx| s.change_font(|st| st.step_font(1.0), cx))),
            )
            .child(
                w::button(&t, "font-reset", tr("Reset")).on_click(cx.listener(|s, _, _, cx| {
                    s.change_font(|st| st.terminal_font_size = FONT_DEFAULT, cx)
                })),
            );
        let reduce = self.settings.reduce_motion;
        let show_status = self.settings.show_status_line;
        let sharp = self.settings.sharp_text;
        let window_blur = self.settings.window_blur;
        let mut appearance_modes = div().flex().gap(px(6.));
        for (id, label, mode) in [
            ("appearance-system", tr("System"), AppearanceMode::System),
            ("appearance-light", tr("Light"), AppearanceMode::Light),
            ("appearance-dark", tr("Dark"), AppearanceMode::Dark),
        ] {
            let selected = self.settings.appearance == mode;
            appearance_modes = appearance_modes.child(
                w::button(&t, id, label)
                    .when(selected, |el| {
                        el.bg(t.ink(0.18)).font_weight(gpui::FontWeight::MEDIUM)
                    })
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.update_settings(|st| st.appearance = mode, cx);
                        s.apply_appearance(cx);
                    })),
            );
        }
        let mut language_modes = div().flex().gap(px(6.));
        for (id, label, mode) in [
            ("language-system", tr("System"), Language::System),
            ("language-english", "English", Language::English),
            ("language-chinese", "中文", Language::Chinese),
        ] {
            let selected = self.settings.language == mode;
            language_modes = language_modes.child(
                w::button(&t, id, label)
                    .when(selected, |el| {
                        el.bg(t.ink(0.18)).font_weight(gpui::FontWeight::MEDIUM)
                    })
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.update_settings(|st| st.language = mode, cx);
                        s.apply_language(cx);
                    })),
            );
        }
        w::page_column()
            .child(w::page_header(&t, tr("Appearance")))
            .child(w::section(
                &t,
                tr("Language"),
                w::card(&t).child(w::row(
                    &t,
                    true,
                    tr("Language"),
                    Some(tr("System uses the language macOS lists first").into()),
                    language_modes,
                )),
            ))
            .child(w::section(
                &t,
                tr("Theme"),
                w::card(&t)
                .child(w::row(
                    &t,
                    true,
                    tr("Appearance"),
                    Some(
                        tr("System follows macOS; the default terminal colours follow it too")
                            .into(),
                    ),
                    appearance_modes,
                ))
                .child(w::row(
                    &t,
                    false,
                    tr("Show status line"),
                    Some(tr("Host, connection state and session time under the terminal").into()),
                    div()
                        .id("toggle-status-line")
                        .switch(tr("Show status line"), show_status)
                        .cursor_pointer()
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.update_settings(|st| st.show_status_line = !st.show_status_line, cx)
                        }))
                        .child(w::toggle(&t, show_status, "status-line")),
                ))
                .child(w::row(
                    &t,
                    false,
                    tr("Sharper text on standard displays"),
                    Some(
                        tr("On 1x monitors: whole-pixel sizes, thinner icons, an opaque window and stronger muted text")
                            .into(),
                    ),
                    div()
                        .id("toggle-sharp-text")
                        .switch(tr("Sharper text on standard displays"), sharp)
                        .cursor_pointer()
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.update_settings(|st| st.sharp_text = !st.sharp_text, cx)
                        }))
                        .child(w::toggle(&t, sharp, "sharp-text")),
                ))
                .when(crate::theme::BLURS_BEHIND, |el| {
                    el.child(w::row(
                        &t,
                        false,
                        tr("Translucent window"),
                        Some(
                            tr("Blur the desktop behind the window when no wallpaper is set; off keeps it opaque and uses less memory")
                                .into(),
                        ),
                        div()
                            .id("toggle-window-blur")
                            .switch(tr("Translucent window"), window_blur)
                            .cursor_pointer()
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.update_settings(|st| st.window_blur = !st.window_blur, cx)
                            }))
                            .child(w::toggle(&t, window_blur, "window-blur")),
                    ))
                })
                .child(w::row(
                    &t,
                    false,
                    tr("Terminal theme"),
                    Some(
                        tr("For every host without its own; set one per host from the sidebar").into(),
                    ),
                    w::button(
                        &t,
                        "theme-default",
                        self.settings
                            .terminal_theme
                            .clone()
                            .unwrap_or_else(|| crate::theme::themes::default_name(t.light).to_owned()),
                    )
                    .on_click(cx.listener(|s, _, window, cx| {
                        s.open_theme_picker(super::ThemeTarget::Default, window, cx)
                    })),
                )),
            ))
            .child(w::section(
                &t,
                tr("Terminal text"),
                w::card(&t).child(w::row(
                    &t,
                    true,
                    tr("Font size"),
                    Some(
                        format!(
                            "{FONT_MIN:.0}–{FONT_MAX:.0} pt · {} {} {}",
                            crate::keymap::primary_hint("="),
                            crate::keymap::primary_hint("−"),
                            crate::keymap::primary_hint("0"),
                        )
                        .into(),
                    ),
                    font,
                )),
            ))
            .child(w::section(&t, tr("Wallpaper"), self.wallpaper_card(cx)))
            .child(w::section(
                &t,
                tr("Motion"),
                w::card(&t).child(w::row(
                    &t,
                    true,
                    tr("Reduce motion"),
                    Some(tr("Hold the connecting pulse and sidebar slide still. On by itself when macOS Reduce motion is on").into()),
                    div()
                        .id("toggle-reduce-motion")
                        .switch(tr("Reduce motion"), reduce)
                        .cursor_pointer()
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.update_settings(|st| st.reduce_motion = !st.reduce_motion, cx);
                            crate::platform::apply_reduce_motion(s.settings.reduce_motion, cx);
                        }))
                        .child(w::toggle(&t, reduce, "reduce-motion")),
                )),
            ))
    }

    fn shortcuts_page(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = self.theme;
        let keymap = cx.global::<Keymap>().clone();
        let mut card = w::card(&t);
        for (ix, id) in ShortcutId::ALL.into_iter().enumerate() {
            let recording = self.recording == Some(id);
            let combo = keymap.combo(id);
            let chip = w::button(
                &t,
                ("shortcut", ix),
                if recording {
                    tr("Press keys…").to_owned()
                } else {
                    badge(combo)
                },
            )
            .min_w(px(88.))
            .when(recording, |el| el.border_1().border_color(t.accent))
            .on_click(cx.listener(move |s, _, window, cx| s.start_recording(id, window, cx)));
            let mut control = div().flex().items_center().gap(px(6.)).child(chip);
            if !keymap.is_default(id) {
                control =
                    control.child(w::button(&t, ("shortcut-reset", ix), tr("Reset")).on_click(
                        cx.listener(move |s, _, _, cx| {
                            s.set_shortcut(id, id.default_combo(), cx);
                        }),
                    ));
            }
            card = card.child(w::row(&t, ix == 0, id.label(), None, control));
        }
        w::page_column()
            .child(w::page_header(&t, tr("Shortcuts")))
            .child(w::page_subtitle(
                &t,
                if cfg!(target_os = "macos") {
                    "Click a shortcut, then press the new keys. Esc cancels, ⌫ unbinds. Keys without \
                     ⌘, ⌃ or ⌥ always go to the remote shell."
                } else {
                    "Click a shortcut, then press the new keys. Esc cancels, Backspace unbinds. \
                     Keys without Ctrl or Alt always go to the remote shell."
                },
            ))
            .when_some(self.record_notice.clone(), |el, notice| {
                el.child(w::page_subtitle(&t, notice).text_color(t.danger))
            })
            .child(w::section(&t, tr("App"), card))
            .child(w::section(
                &t,
                tr("Fixed"),
                w::card(&t)
                    .child(w::row(
                        &t,
                        true,
                        tr("Switch to tab 1–9"),
                        None,
                        div().text_sm().text_color(t.muted).child(if cfg!(target_os = "macos") {
                            "⌘1 … ⌘9"
                        } else {
                            "Alt+1 … Alt+9"
                        }),
                    ))
                    .child(w::row(
                        &t,
                        false,
                        tr("Next / previous tab"),
                        None,
                        div().text_sm().text_color(t.muted).child(if cfg!(target_os = "macos") {
                            "⌃Tab  ⌃⇧Tab"
                        } else {
                            "Ctrl+Tab  Ctrl+Shift+Tab"
                        }),
                    ))
                    .child(w::row(
                        &t,
                        false,
                        if cfg!(target_os = "macos") {
                            tr("Quit, hide, minimise")
                        } else {
                            tr("Quit")
                        },
                        None,
                        div().text_sm().text_color(t.muted).child(if cfg!(target_os = "macos") {
                            "⌘Q  ⌘H  ⌘M"
                        } else {
                            "Ctrl+Shift+Q"
                        }),
                    )),
            ))
    }

    fn start_recording(&mut self, id: ShortcutId, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = Some(id);
        self.record_notice = None;
        window.focus(&self.focus, cx);
        // Bound actions run before element key listeners, so intercept first: the chord being
        // recorded must not also fire whatever it is bound to now.
        let shell = cx.entity().downgrade();
        self.record_interceptor = Some(cx.intercept_keystrokes(move |event, _, cx| {
            let ks = &event.keystroke;
            let m = &ks.modifiers;
            let outcome = record(&ks.key, m.control, m.alt, m.shift, m.platform);
            let _ = shell.update(cx, |s, cx| s.on_recorded(outcome, cx));
            cx.stop_propagation();
        }));
        cx.notify();
    }

    fn on_recorded(&mut self, outcome: Record, cx: &mut Context<Self>) {
        let Some(id) = self.recording else {
            return;
        };
        match outcome {
            Record::Ignored => return,
            Record::Cancelled => {}
            Record::Cleared => self.set_shortcut(id, "", cx),
            Record::Set(combo) => match cx.global::<Keymap>().refusal(id, &combo) {
                Some(why) => self.record_notice = Some(why),
                None => self.set_shortcut(id, &combo, cx),
            },
        }
        self.recording = None;
        self.record_interceptor = None;
        cx.notify();
    }

    /// Saves `keymap.json` and re-installs every binding.
    fn set_shortcut(&mut self, id: ShortcutId, combo: &str, cx: &mut Context<Self>) {
        cx.global_mut::<Keymap>().set(id, combo);
        if let Some(dir) = crate::settings::dir()
            && let Err(e) = cx.global::<Keymap>().save(&dir)
        {
            self.record_notice = Some(fill("Could not save keymap.json: {}", &[&e.to_string()]));
        }
        crate::keymap::apply(cx);
        cx.notify();
    }

    fn connections_page(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = self.theme;
        let mut list = w::card(&t);
        if self.connections.is_empty() {
            list = list.child(w::row(
                &t,
                true,
                tr("No connections yet"),
                Some(tr("Hosts from ~/.ssh/config are listed in the sidebar").into()),
                div(),
            ));
        }
        for (ix, c) in self.connections.iter().enumerate() {
            let address = if c.port == 22 {
                format!("{}@{}", c.user, c.host)
            } else {
                format!("{}@{}:{}", c.user, c.host, c.port)
            };
            let confirming = self.confirm_delete == Some(ix);
            let actions = div()
                .flex()
                .gap(px(6.))
                .child(
                    w::button(&t, ("settings-edit", ix), tr("Edit")).on_click(cx.listener(
                        move |s, _, window, cx| {
                            let draft = s.connection(ix);
                            s.open_form(Some(ix), draft, window, cx);
                        },
                    )),
                )
                .child(
                    w::button(
                        &t,
                        ("settings-delete", ix),
                        if confirming {
                            tr("Delete?")
                        } else {
                            tr("Remove")
                        },
                    )
                    .when(confirming, |el| el.text_color(t.danger))
                    .on_click(cx.listener(move |s, _, _, cx| s.delete_connection(ix, cx))),
                );
            list = list.child(w::row(
                &t,
                ix == 0,
                c.name.clone(),
                Some(SharedString::from(address)),
                actions,
            ));
        }
        w::page_column()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(w::page_header(&t, tr("Connections")))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(
                                w::button(&t, "settings-import", tr("Import from ~/.ssh/config…"))
                                    .on_click(
                                        cx.listener(|s, _, window, cx| s.open_import(window, cx)),
                                    ),
                            )
                            .child(w::button(&t, "settings-new", tr("New connection")).on_click(
                                cx.listener(|s, _, window, cx| s.open_form(None, None, window, cx)),
                            )),
                    ),
            )
            .child(w::page_subtitle(
                &t,
                fill(
                    "{} saved in kite. Import copies hosts out of ~/.ssh/config once; it is never read or written afterwards.",
                    &[&self.connections.len().to_string()],
                ),
            ))
            .when_some(self.store_error.clone(), |el, e| {
                el.child(w::page_subtitle(&t, e).text_color(t.danger))
            })
            .child(w::section(&t, tr("Saved in kite"), list))
    }
}

impl Shell {
    fn about_page(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = self.theme;
        let on = self.settings.check_for_updates;
        w::page_column()
            .child(w::page_header(&t, tr("About")))
            .child(w::section(
            &t,
            "Kite",
            w::card(&t)
                .child(w::row(
                    &t,
                    true,
                    tr("Version"),
                    Some(env!("CARGO_PKG_VERSION").into()),
                    div(),
                ))
                .child(w::row(
                    &t,
                    true,
                    tr("Check for updates"),
                    Some(
                        tr("Once a day, ask GitHub for a newer release; kite never installs one")
                            .into(),
                    ),
                    div()
                        .id("toggle-check-updates")
                        .switch(tr("Check for updates"), on)
                        .cursor_pointer()
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.update_settings(
                                |st| st.check_for_updates = !st.check_for_updates,
                                cx,
                            );
                        }))
                        .child(w::toggle(&t, on, "check-updates")),
                ))
                .child(w::row(
                    &t,
                    false,
                    tr("Logs"),
                    Some(crate::settings::log_dir_label().into()),
                    div(),
                )),
        ))
    }
}
