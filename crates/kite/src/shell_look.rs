//! How the window looks: colour schemes per host, light and dark appearance, restyling open
//! terminals, and the status line under the terminal.
//!
//! The window's frame: one composition, the same with and without a wallpaper.
//!
//! Layers, back to front, each drawn once:
//! 1. the base at the window root: the opaque shell colour, or, with no wallpaper, the macOS
//!    frost (the window's own blurred appearance behind the shell colour at `GLASS_ALPHA`);
//! 2. when a wallpaper fills the window, its blurred copy, cover-fit to the window, one element;
//! 3. tiles side by side, edge to edge, with no margins, cards or gaps: the titlebar band across
//!    the top, then sidebar | main (or Settings nav | page). A tile is only a tint at the panel
//!    alpha over the layers behind it, so panels never carry a picture of their own;
//! 4. one neutral 1 px hairline on the sidebar's right edge, which moves with it as it tweens.
//!
//! The sharp picture is only the empty view's hero, laid out inside the main tile. Sidebar width
//! is held in whole device pixels so no edge lands between two.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Hsla, IntoElement, ParentElement, Window, WindowBackgroundAppearance, div,
    px,
};

use super::*;
use crate::i18n::tr;

impl Shell {
    /// The scheme a host's tabs use: its own, else the default, else zeron's (`None`).
    pub(super) fn scheme_for(&self, alias: &str) -> Option<&'static crate::theme::themes::Scheme> {
        self.settings
            .host_themes
            .get(alias)
            .or(self.settings.terminal_theme.as_ref())
            .and_then(|name| crate::theme::themes::find(name))
    }

    /// Switches the palette when the chosen appearance (or the system's, for System) differs
    /// from the one showing, and restyles everything that holds a colour: open terminals, text
    /// fields, the wallpaper tint.
    pub(crate) fn apply_appearance(&mut self, cx: &mut Context<Self>) {
        let light = self.settings.appearance.is_light(self.system_light);
        if light == self.theme.light {
            return;
        }
        self.theme = Theme::zeron(light);
        self.refresh_theme(cx);
        self.install_input_colors(cx);
        self.restyle_tabs(cx);
        cx.notify();
    }

    /// Switches the interface language and the macOS menu bar when the setting says to.
    pub(crate) fn apply_language(&mut self, cx: &mut Context<Self>) {
        if !crate::i18n::install(self.settings.language) {
            return;
        }
        crate::chrome::install_menus(cx);
        self.retitle_fields(cx);
        cx.notify();
    }

    /// Placeholders are fixed when a field is created, so a language change rewrites them.
    fn retitle_fields(&self, cx: &mut Context<Self>) {
        let mut set = |input: &gpui::Entity<crate::text_input::TextInput>, label: &'static str| {
            input.update(cx, |field, cx| {
                field.set_placeholder(crate::i18n::tr(label), cx);
            });
        };
        set(&self.hostlist.search, tr("Search hosts"));
        if let Some(ui) = &self.vault_ui {
            set(&ui.unlock, tr("Vault passphrase"));
            set(&ui.current, tr("Current passphrase"));
            set(&ui.new, tr("New passphrase"));
            set(&ui.confirm, tr("Repeat new passphrase"));
            set(&ui.pin, tr("Vault PIN"));
            set(&ui.pin_new, tr("New PIN, 4 to 8 digits"));
            set(&ui.pin_repeat, tr("Repeat PIN"));
            set(&ui.key_name, tr("Key name, e.g. laptop"));
        }
        if let Some(ui) = &self.sync_ui {
            set(&ui.token_input, tr("ghp_… with the gist scope"));
            set(&ui.pass_input, tr("Vault passphrase"));
        }
    }

    /// Standard-density mode for this frame, from the monitor the window is on right now.
    pub(super) fn sync_density(&self, window: &Window) {
        crate::theme::set_low_dpi(
            self.settings.sharp_text && window.scale_factor() < crate::theme::LOW_DPI_BELOW,
        );
    }

    /// Text fields read their colours from this global, so open ones follow the theme.
    pub(crate) fn install_input_colors(&self, cx: &mut Context<Self>) {
        let t = self.theme;
        cx.set_global(crate::text_input::InputColors {
            text: t.text,
            placeholder: t.faint,
            cursor: t.accent,
            selection: t.accent.opacity(0.35),
        });
    }

    pub(super) fn terminal_theme(&self, alias: &str) -> kite_term::TerminalTheme {
        let mut theme = self
            .theme
            .terminal(self.settings.terminal_font_size, self.scheme_for(alias));
        if let Some(family) = &self.settings.terminal_font_family {
            theme.font_family = family.clone().into();
        }
        // Over a full-window wallpaper the terminal is a panel too: the picture shows through.
        if let Some(alpha) = self.window_fill() {
            theme.background_alpha = alpha;
        }
        theme
    }

    /// Re-applies font and scheme to every open terminal; each re-measures its cells and the
    /// remote side is told the new grid size.
    pub(super) fn restyle_tabs(&self, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            for pane in &tab.panes {
                let theme = self.terminal_theme(&pane.alias);
                let view = pane.session.read(cx).view.clone();
                view.update(cx, |v, cx| v.set_theme(theme, cx));
            }
        }
    }

    pub(super) fn change_font(
        &mut self,
        change: impl FnOnce(&mut Settings),
        cx: &mut Context<Self>,
    ) {
        self.update_settings(change, cx);
        self.restyle_tabs(cx);
    }

    /// Background of the main panel: the active tab's scheme, so no seam shows around it.
    pub(super) fn panel_background(&self) -> gpui::Hsla {
        match self.tabs.get(self.active) {
            Some(tab) => self.terminal_theme(tab.focused_alias()).background,
            None => self.theme.terminal_background,
        }
    }

    /// Syncs and returns the isolated status bar entity when the setting is on.
    pub(super) fn render_status_line(
        &mut self,
        show_workspace: bool,
        bg: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> Option<Entity<super::status_bar::StatusBar>> {
        let session = (show_workspace && self.settings.show_status_line)
            .then(|| self.tabs.get(self.active).map(|t| t.session().clone()))
            .flatten();
        let Some(session) = session else {
            self.status_bar.update(cx, |bar, _| {
                bar.sync(self.theme, bg, None, false, 0, false);
            });
            return None;
        };
        let forwards_count = self.shared_forward_count(&session, cx);
        let sftp_open = self.panels.sftp.is_some();
        let theme = self.theme;
        self.status_bar.update(cx, |bar, _| {
            bar.sync(theme, bg, Some(session), sftp_open, forwards_count, true);
        });
        Some(self.status_bar.clone())
    }
}

/// Text over the wallpaper hero sits on a plate of the panel colour, so it never depends on
/// what the picture holds.
pub(super) fn plate(el: gpui::Div, panel: gpui::Hsla) -> gpui::Div {
    use gpui::Styled;
    el.px(gpui::px(20.))
        .py(gpui::px(12.))
        .rounded(gpui::px(12.))
        .bg(panel.opacity(0.85))
}

/// `value` (logical px) rounded to a whole number of device pixels at `scale`.
pub(crate) fn snap(value: f32, scale: f32) -> f32 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    (value * scale).round() / scale
}

/// What the frame remembers between frames.
pub(super) struct FrameState {
    /// The window's scale factor at its last render, for snapping widths outside `render`.
    pub(super) scale: f32,
    /// The window appearance last set, so it is only touched when it changes.
    blurred: Option<bool>,
}

impl Default for FrameState {
    fn default() -> Self {
        Self {
            scale: 1.0,
            blurred: None,
        }
    }
}

impl Shell {
    /// Records the scale factor and switches the window between its blurred appearance (no
    /// wallpaper: the desktop shows through the frost) and an opaque one (a wallpaper is set:
    /// nothing behind the window may show).
    pub(super) fn sync_frame(&mut self, window: &mut Window) {
        self.frame.scale = window.scale_factor();
        let blurred = crate::theme::BLURS_BEHIND && !self.has_wallpaper();
        if self.frame.blurred != Some(blurred) {
            self.frame.blurred = Some(blurred);
            window.set_background_appearance(if blurred {
                WindowBackgroundAppearance::Blurred
            } else {
                WindowBackgroundAppearance::Opaque
            });
        }
    }

    /// The window root's colour: always opaque under a wallpaper, frost otherwise.
    pub(super) fn window_base(&self) -> Hsla {
        if self.has_wallpaper() {
            self.theme.shell
        } else {
            self.theme.surface()
        }
    }

    /// The blurred picture across the window, drawn once; empty unless the picture fills it.
    pub(super) fn backdrop(&self) -> Vec<AnyElement> {
        use gpui::StyledImage as _;
        let Some(blurred) = self.blurred_backdrop() else {
            return Vec::new();
        };
        vec![
            gpui::img(blurred)
                .absolute()
                .inset_0()
                .size_full()
                .object_fit(gpui::ObjectFit::Cover)
                .opacity(self.settings.wallpaper_hero_opacity)
                .into_any_element(),
        ]
    }

    /// The tint a tile of colour `colour` lays over what is behind it: the panel alpha over a
    /// full-window picture, nothing otherwise (the window base already is that colour).
    pub(super) fn tile_tint(&self, colour: Hsla) -> Option<Hsla> {
        self.window_fill().map(|alpha| colour.opacity(alpha))
    }

    /// A fixed-width tile at the left of the body (the sidebar, or Settings' nav) with the
    /// separator on its right edge. Clipped to `width`, so collapsing never reflows its rows.
    pub(super) fn side_tile(&self, width: f32, content: impl IntoElement) -> gpui::Div {
        div()
            .flex_none()
            .relative()
            .h_full()
            .w(px(width))
            .overflow_hidden()
            .when_some(self.tile_tint(self.theme.shell), |el, tint| el.bg(tint))
            .child(content)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w(px(1.))
                    .bg(self.theme.hairline),
            )
    }

    /// The main tile: fills what the side tile leaves. When it shows a terminal over a
    /// full-window wallpaper it paints nothing, as the terminal draws its own glass at the same
    /// alpha; anything else it shows (Settings, the empty view) sits on the tile's tint.
    pub(super) fn main_tile(&self, panel: Hsla) -> gpui::Div {
        let shows_terminal = !self.tabs.is_empty() && self.settings_page.is_none();
        let terminal_glass = shows_terminal && self.window_fill().is_some();
        div()
            .flex_1()
            .min_w_0()
            .relative()
            .overflow_hidden()
            .flex()
            .flex_col()
            .when(!terminal_glass, |el| {
                el.bg(self.tile_tint(panel).unwrap_or(panel))
            })
    }
}

impl Shell {
    /// The main panel's content: the active tab's panes, or the empty view, which carries the
    /// wallpaper `hero` (the picture filling the tile) under its text.
    pub(super) fn panel_content(
        &self,
        hero: Option<AnyElement>,
        panel_bg: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(tab) = self.tabs.get(self.active) {
            return self.render_panes(tab, cx);
        }
        let t = self.theme;
        let keymap = cx.global::<crate::keymap::Keymap>();
        let hint = |id: crate::keymap::ShortcutId, label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .min_w(px(44.))
                        .flex()
                        .justify_end()
                        .text_sm()
                        .text_color(t.text)
                        .child(gpui::SharedString::from(crate::keymap::badge(
                            keymap.combo(id),
                        ))),
                )
                .child(div().text_sm().text_color(t.muted).child(label))
        };
        let (title, detail): (gpui::SharedString, Option<gpui::SharedString>) = match &self.error {
            Some(e) => ("Could not open that".into(), Some(e.clone().into())),
            None if self.hosts.is_empty() => (
                "No hosts yet".into(),
                Some("Add a connection, or copy your hosts over from ~/.ssh/config".into()),
            ),
            None => ("No session open".into(), None),
        };
        // Over the picture the text sits below its upper part, on a plate of the panel colour,
        // so it never depends on what the picture holds.
        let on_picture = hero.is_some();
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(16.))
            .children(hero)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .when(on_picture, |el| plate(el, panel_bg))
                    .child(div().text_size(px(15.)).text_color(t.text).child(title))
                    .when_some(detail, |el, d| {
                        el.child(div().text_sm().text_color(t.muted).child(d))
                    }),
            )
            .when(self.hosts.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            super::connections_ui::button(
                                "empty-main-new",
                                crate::i18n::tr("New connection"),
                                true,
                                &t,
                            )
                            .on_click(cx.listener(|s, _, w, cx| s.open_form(None, None, w, cx))),
                        )
                        .child(
                            super::connections_ui::button(
                                "empty-main-import",
                                crate::i18n::tr("Import from ~/.ssh/config…"),
                                false,
                                &t,
                            )
                            .on_click(cx.listener(|s, _, w, cx| s.open_import(w, cx))),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .when(on_picture, |el| plate(el, panel_bg))
                    .child(hint(crate::keymap::ShortcutId::HostPicker, "Find a host"))
                    .child(hint(
                        crate::keymap::ShortcutId::NewConnection,
                        crate::i18n::tr("New connection"),
                    ))
                    .child(hint(
                        crate::keymap::ShortcutId::Settings,
                        crate::i18n::tr("Settings"),
                    )),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::snap;

    #[test]
    fn snap_lands_on_whole_device_pixels() {
        for (value, scale) in [(256.4, 1.0), (256.4, 2.0), (257.3, 2.0), (300.7, 1.5)] {
            let device = snap(value, scale) * scale;
            assert!((device - device.round()).abs() < 1e-3, "{value}@{scale}");
        }
    }

    #[test]
    fn snap_rounds_to_the_nearest_not_down() {
        assert_eq!(snap(256.4, 1.0), 256.0);
        assert_eq!(snap(256.6, 1.0), 257.0);
        // At 2x half a point is a whole device pixel; a quarter rounds to the nearer edge.
        assert_eq!(snap(256.5, 2.0), 256.5);
        assert_eq!(snap(256.25, 2.0), 256.5);
        assert_eq!(snap(256.2, 2.0), 256.0);
    }

    #[test]
    fn snap_survives_a_bad_scale() {
        assert_eq!(snap(10.4, 0.0), 10.0);
        assert_eq!(snap(10.4, f32::NAN), 10.0);
    }
}
