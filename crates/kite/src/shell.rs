// Adapted from zeron crates/ui/src/lib.rs (window options) (MIT).
//! The main window: frosted shell, titlebar with tabs, host sidebar and the main panel.

use gpui::prelude::FluentBuilder;
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement,
    MouseButton, MouseUpEvent, ParentElement, Render, StatefulInteractiveElement, Styled,
    Subscription, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowOptions, actions, div, point, px, size,
};
#[cfg(debug_assertions)]
use kite_ssh::ConnectSpec;
use kite_ssh::HostEntry;

use crate::chrome::{self, DragGhost, SidebarResize, WidthTween};
use crate::connections::{self, Connection};
use crate::picker::{self, Picker, ToggleHostPicker};
use crate::session::{Launch, Session, Status};
use crate::settings::{self, SIDEBAR_DEFAULT, Settings};
use crate::sidebar;
use crate::split::{self, PaneId};
use crate::tabs::{self, ActivateTab, CloseTab, NextTab, PrevTab};
use crate::theme::Theme;

actions!(
    tern,
    [
        ToggleSidebar,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize,
        NewConnection,
        SplitRight,
        SplitDown,
        FocusPaneLeft,
        FocusPaneRight,
        FocusPaneUp,
        FocusPaneDown,
        OpenSettings,
        SaveLayout
    ]
);

#[path = "shell_broadcast.rs"]
mod broadcast_ui;
#[path = "shell_forwards_shared.rs"]
mod forwards_shared;
#[path = "shell_forwards.rs"]
mod forwards_ui;
#[path = "shell_sftp.rs"]
mod sftp_ui;
pub(crate) use forwards_ui::FillPassword;
pub(crate) use sftp_ui::ToggleSftp;
#[path = "shell_wallpaper_gallery.rs"]
mod gallery_ui;
#[path = "shell_look.rs"]
mod look;
#[path = "shell_menu.rs"]
mod menu;
#[path = "shell_open.rs"]
mod open;
#[path = "shell_settings.rs"]
mod settings_ui;
#[path = "shell_split_detach.rs"]
mod split_detach;
#[path = "shell_status_bar.rs"]
mod status_bar;
pub(crate) use split_detach::DraggedPane;
#[path = "shell_split_drag.rs"]
mod split_drag;
#[path = "shell_split_merge.rs"]
mod split_merge;
#[path = "shell_sync.rs"]
mod sync_ui;
#[path = "shell_tabs.rs"]
mod tabs_ui;
#[path = "shell_term_options.rs"]
mod term_options;
#[path = "shell_toast.rs"]
mod toast;
#[path = "shell_update.rs"]
mod update_ui;
#[path = "shell_vault_page.rs"]
mod vault_page_ui;
#[path = "shell_vault.rs"]
mod vault_ui;
#[path = "shell_wallpaper.rs"]
mod wallpaper_ui;

pub(crate) use toast::CubicBezier;

pub(crate) use toast::{Kind as ToastKind, Toast};
#[path = "shell_themes.rs"]
mod themes_ui;

pub(crate) use themes_ui::ThemeTarget;

#[path = "shell_connections.rs"]
pub(crate) mod connections_ui;
#[path = "shell_forwards_view.rs"]
mod forwards_view;
#[path = "shell_groups.rs"]
mod groups_ui;
#[path = "shell_hostlist.rs"]
mod hostlist;
#[path = "shell_import.rs"]
mod import_ui;
#[path = "shell_layouts.rs"]
mod layouts_ui;
#[path = "shell_local_cwd.rs"]
mod local_cwd;
#[path = "shell_snippets.rs"]
mod snippets_ui;

pub(crate) use hostlist::FocusHostSearch;
pub(crate) use snippets_ui::ToggleSnippets;

pub fn open_main_window(cx: &mut App) -> anyhow::Result<WindowHandle<Shell>> {
    let bounds = open::start_bounds(cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(900.), px(600.))),
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.), px(14.))),
        }),
        app_owns_titlebar_drag: true,
        window_background: if crate::theme::BLURS_BEHIND {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Opaque
        },
        app_id: Some("kite".into()),
        ..Default::default()
    };
    let recent = settings::dir()
        .map(|d| hostlist::Recent::load(&d))
        .unwrap_or_default();
    let (connections, store_error) = match settings::dir().map(|d| connections::load(&d)) {
        Some(Ok(list)) => (list, None),
        Some(Err(e)) => {
            tracing::warn!(error = %e, "hosts_json_unreadable");
            (Vec::new(), Some(e.to_string()))
        }
        None => (Vec::new(), None),
    };
    let window = cx.open_window(options, |_, cx| {
        cx.new(|cx| {
            let theme = Theme::zeron_dark();
            let weak = cx.weak_entity();
            let mut shell = Shell {
                focus: cx.focus_handle(),
                settings: settings::dir()
                    .map(|d| Settings::load(&d))
                    .unwrap_or_default(),
                theme,
                hosts: Vec::new(),
                connections,
                store_error,
                form: None,
                confirm_delete: None,
                tabs: Vec::new(),
                renaming: None,
                restoring: false,
                tabs_unreadable: false,
                next_tab: 0,
                next_pane: 0,
                broadcast: None,
                broadcast_picker: None,
                panels: Default::default(),
                forward_claims: Vec::new(),
                active: 0,
                split_preview: None,
                split_ratio_dirty: false,
                split_preview_kind: None,
                pane_insert: None,
                error: None,
                picker: None,
                sidebar_tween: None,
                settings_page: None,
                theme_picker: None,
                recording: None,
                record_notice: None,
                record_interceptor: None,
                sync_ui: None,
                auto_sync: sync_ui::AutoSync::default(),
                vault_ui: None,
                last_input: std::time::Instant::now(),
                toasts: toast::Toasts::new(),
                tab_scroll: gpui::ScrollHandle::new(),
                context_menu: None,
                hostlist: hostlist::HostList::new(&theme, recent, cx),
                group_prompt: None,
                snippets: snippets_ui::SnippetsUi::load(),
                layouts: layouts_ui::State::load(),
                import: None,
                wp: wallpaper_ui::State::default(),
                frame: look::FrameState::default(),
                system_light: false,
                status_bar: cx.new(|cx| status_bar::StatusBar::new(weak, cx)),
            };
            shell.refresh_hosts();
            shell
        })
    })?;
    window.update(cx, |shell, window, cx| {
        crate::platform::apply_reduce_motion(shell.settings.reduce_motion, cx);
        // macOS posts no notification gpui forwards, so re-read the preference whenever tern
        // comes to the front: the user changes it in System Settings, then switches back.
        cx.observe_window_activation(window, |shell, window, cx| {
            crate::platform::apply_reduce_motion(shell.settings.reduce_motion, cx);
            if window.is_window_active() {
                shell.apply_language(cx);
                for pane in shell.tabs.iter().flat_map(|t| &t.panes) {
                    pane.session.update(cx, |s, cx| s.nudge(cx));
                }
            }
        })
        .detach();
        shell.system_light = is_light(window.appearance());
        shell.apply_appearance(cx);
        shell.apply_language(cx);
        shell.install_input_colors(cx);
        cx.observe_window_appearance(window, |shell, window, cx| {
            shell.system_light = is_light(window.appearance());
            shell.apply_appearance(cx);
        })
        .detach();
        shell.check_for_updates(cx);
        shell.start_auto_sync(window, cx);
    })?;
    // With no tab open nothing else holds focus, and gpui only dispatches key bindings along
    // the focused element's path, so the shell itself must be focused for ⌘K to work.
    window.update(cx, |shell, window, cx| window.focus(&shell.focus, cx))?;
    window.update(cx, |shell, window, cx| shell.restore_tabs(window, cx))?;
    window.update(cx, |shell, window, cx| {
        shell.install_local_cwd_hooks(window, cx)
    })?;
    window.update(cx, |shell, _, cx| shell.watch_idle(cx))?;
    Ok(window)
}

/// macOS's light appearances; the dark ones and anything new count as dark.
fn is_light(appearance: gpui::WindowAppearance) -> bool {
    matches!(
        appearance,
        gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
    )
}

pub struct Shell {
    focus: FocusHandle,
    settings: Settings,
    theme: Theme,
    /// What the sidebar and picker list: tern's own connections, in `hosts.json` order.
    hosts: Vec<HostEntry>,
    connections: Vec<Connection>,
    /// Set when `hosts.json` exists but cannot be read; saving is refused so it is not lost.
    store_error: Option<String>,
    form: Option<connections_ui::ConnectionForm>,
    confirm_delete: Option<usize>,
    tabs: Vec<Tab>,
    /// The tab whose title is being edited in the strip.
    renaming: Option<tabs_ui::Rename>,
    /// True while the last run's tabs are being reopened, so half a list is never saved.
    restoring: bool,
    /// Set when `tabs.json` is newer or corrupt, so it is not overwritten.
    tabs_unreadable: bool,
    /// Input typed in one tab also going to others, and the list that picks them.
    next_tab: broadcast_ui::TabId,
    next_pane: PaneId,
    broadcast: Option<broadcast_ui::Broadcast>,
    broadcast_picker: Option<broadcast_ui::BroadcastPicker>,
    panels: forwards_ui::Panels,
    /// Forwards a session has been asked to start and has not finished yet.
    forward_claims: Vec<forwards_shared::Claim>,
    active: usize,
    /// The pane half a dragged tab or pane would split into, while the pointer is over it.
    split_preview: Option<split_drag::SplitPreview>,
    /// A split divider moved during a drag and has not been written yet.
    split_ratio_dirty: bool,
    /// Which drag last wrote `split_preview`, so a tab merge and a pane rearrange do not share a hint.
    split_preview_kind: Option<split_drag::PreviewKind>,
    /// Where a pane dragged by its title would be inserted in the strip.
    pane_insert: Option<split_detach::PaneInsertHint>,
    error: Option<String>,
    picker: Option<Picker>,
    sidebar_tween: Option<WidthTween>,
    settings_page: Option<settings_ui::Section>,
    theme_picker: Option<themes_ui::ThemePicker>,
    /// The shortcut being recorded in Settings → Shortcuts, and why the last one was refused.
    recording: Option<crate::keymap::ShortcutId>,
    record_notice: Option<String>,
    record_interceptor: Option<Subscription>,
    sync_ui: Option<sync_ui::SyncUi>,
    auto_sync: sync_ui::AutoSync,
    vault_ui: Option<vault_ui::VaultUi>,
    /// The last key or mouse input to the window, for the vault's idle lock.
    last_input: std::time::Instant,
    toasts: toast::Toasts,
    tab_scroll: gpui::ScrollHandle,
    context_menu: Option<menu::ContextMenu>,
    hostlist: hostlist::HostList,
    group_prompt: Option<groups_ui::Prompt>,
    snippets: snippets_ui::SnippetsUi,
    layouts: layouts_ui::State,
    import: Option<import_ui::ImportSheet>,
    wp: wallpaper_ui::State,
    frame: look::FrameState,
    /// Whether macOS is in its light appearance; what `Appearance: System` follows.
    system_light: bool,
    status_bar: Entity<status_bar::StatusBar>,
}

/// Debug builds: keep scripted logins out of the real ~/.ssh/known_hosts.
#[cfg(debug_assertions)]
fn with_dev_known_hosts(spec: ConnectSpec) -> ConnectSpec {
    ConnectSpec {
        known_hosts: std::env::var_os("KITE_KNOWN_HOSTS")
            .map(Into::into)
            .or(spec.known_hosts),
        ..spec
    }
}

/// One terminal in a tab: a connection, and the host it was opened as.
struct Pane {
    id: PaneId,
    /// A tab can hold panes from different hosts, so the host lives here.
    alias: String,
    /// Directory a local shell was last seen in. SSH panes leave this empty.
    cwd: Option<std::path::PathBuf>,
    session: Entity<Session>,
    _repaint: Subscription,
    _bell: Subscription,
    _notes: Subscription,
}

struct Tab {
    id: broadcast_ui::TabId,
    /// The user's name for the tab. It belongs to the tab, not the session, so a reconnect
    /// keeps it.
    title: Option<String>,
    /// The saved layout this tab updates, when it was opened or saved as one.
    layout_id: Option<String>,
    /// How the panes are laid out, and which one has the keyboard.
    tree: split::Node,
    focused: PaneId,
    panes: Vec<Pane>,
}

impl Tab {
    /// The focused pane's session: what typing reaches, and what the strip's dot follows.
    fn session(&self) -> &Entity<Session> {
        match self.panes.iter().find(|p| p.id == self.focused) {
            Some(pane) => &pane.session,
            // `focused` always names a pane; the first is the fallback if that ever breaks.
            None => &self.panes[0].session,
        }
    }
}

impl Shell {
    pub fn toggle_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        tracing::debug!(open = self.picker.is_some(), "picker_toggle");
        if self.picker.is_some() {
            return self.close_picker(window, cx);
        }
        let picker = Picker::new(cx);
        window.focus(picker.focus(), cx);
        self.picker = Some(picker);
        cx.notify();
    }

    /// Closing hands focus back to the active terminal, so typing resumes where it was.
    pub fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.take().is_some() {
            self.restore_focus(window, cx);
        }
    }

    /// Applies a settings change and writes it out; a failed write is logged, not fatal.
    pub fn update_settings(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        change(&mut self.settings);
        self.settings = self.settings.clone().clamped();
        if let Some(dir) = settings::dir()
            && let Err(e) = self.settings.save(&dir)
        {
            tracing::warn!(error = %e, "settings_save_failed");
        }
        cx.notify();
    }

    fn sidebar_target(&self) -> f32 {
        if self.settings.sidebar_collapsed {
            0.0
        } else {
            look::snap(self.settings.sidebar_width, self.frame.scale)
        }
    }

    /// The sidebar's width this frame: mid-tween while collapsing or expanding.
    fn sidebar_now(&self) -> f32 {
        self.sidebar_tween
            .and_then(|tween| tween.sample(std::time::Instant::now()))
            .map_or_else(
                || self.sidebar_target(),
                |w| look::snap(w, self.frame.scale),
            )
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        let from = self.sidebar_now();
        self.update_settings(|s| s.sidebar_collapsed = !s.sidebar_collapsed, cx);
        self.sidebar_tween = Some(WidthTween::new(from, self.sidebar_target()));
    }

    /// Live drag: follows the pointer without writing the file on every move.
    fn on_sidebar_drag(&mut self, x: f32, cx: &mut Context<Self>) {
        self.settings.sidebar_width = look::snap(chrome::dragged_width(x), self.frame.scale);
        self.settings.sidebar_collapsed = false;
        self.sidebar_tween = None;
        cx.notify();
    }

    fn resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .id("sidebar-resize")
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(self.sidebar_now() - chrome::HANDLE_HALF_WIDTH))
            .w(px(chrome::HANDLE_HALF_WIDTH * 2.0))
            .occlude()
            .cursor_col_resize()
            .on_drag(SidebarResize, |_, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| DragGhost)
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|shell, event: &MouseUpEvent, _, cx| {
                    let reset = event.click_count == 2;
                    shell.update_settings(
                        |s| {
                            if reset {
                                s.sidebar_width = SIDEBAR_DEFAULT;
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

    fn refresh_hosts(&mut self) {
        self.hosts = self
            .connections
            .iter()
            .map(|c| c.entry_in(&self.connections))
            .collect();
        self.hostlist.empty_groups.retain(|name| {
            !self
                .connections
                .iter()
                .any(|connection| connection.group.as_deref() == Some(name.as_str()))
        });
        self.hostlist.layout =
            sidebar::layout_with_empty(&self.connections, &self.hostlist.empty_groups);
    }

    /// Focus back to the active terminal, or the window when there is none.
    fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = match self.tabs.get(self.active) {
            Some(tab) => tab.session().read(cx).view.focus_handle(cx),
            None => self.focus.clone(),
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    fn apply_option_as_meta(&self, cx: &mut Context<Self>) {
        let on = self.settings.option_as_meta;
        for tab in &self.tabs {
            for pane in &tab.panes {
                let view = pane.session.read(cx).view.clone();
                view.update(cx, |v, _| v.set_option_as_meta(on));
            }
        }
    }

    pub fn hosts(&self) -> &[HostEntry] {
        &self.hosts
    }

    pub fn picker_query(&self) -> String {
        self.picker
            .as_ref()
            .map(|p| p.query().to_string())
            .unwrap_or_default()
    }

    pub fn picker_mut(&mut self) -> Option<&mut Picker> {
        self.picker.as_mut()
    }

    fn fail(&mut self, error: String, cx: &mut Context<Self>) {
        tracing::warn!(error = %error, "connect_target_invalid");
        self.error = Some(error);
        cx.notify();
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.clear_split_preview_without_drag(cx);
        self.flush_split_ratio(cx);
        self.clear_pane_insert_without_drag(cx);
        // Whatever held focus can disappear (a field hidden after sync, a closed form or tab);
        // with nothing focused gpui dispatches no keys at all, so the window takes it back.
        if window.focused(cx).is_none() {
            window.focus(&self.focus, cx);
        }
        self.sync_density(window);
        self.sync_wallpaper(cx);
        self.sync_frame(window);
        let t = self.theme;
        let infos = self.tab_infos(cx);
        let active_alias = infos.get(self.active).map(|i| i.alias.clone());
        let insert_at = self.pane_insert.map(|hint| hint.slot);
        let strip = tabs::strip(&infos, self.active, &self.tab_scroll, insert_at, &t, cx);
        let sidebar_now = self.sidebar_now();
        if self.sidebar_tween.is_some() {
            if sidebar_now == self.sidebar_target() {
                self.sidebar_tween = None;
            } else {
                window.request_animation_frame();
            }
        }
        let show_workspace = self.settings_page.is_none();
        let sidebar = if show_workspace && sidebar_now > 0.0 {
            let query = self.search_query(cx);
            let layouts_height = look::snap(
                sidebar::layouts_display_height(
                    self.settings.sidebar_layouts_height,
                    f32::from(window.viewport_size().height),
                ),
                self.frame.scale,
            );
            sidebar::render(
                &sidebar::SidebarState {
                    hosts: &self.hosts,
                    connections: &self.connections,
                    layout: &self.hostlist.layout,
                    open: &infos,
                    active_alias: active_alias.as_deref(),
                    width: self.settings.sidebar_width,
                    collapsed_groups: &self.hostlist.collapsed_groups,
                    layouts: &self.layouts.items,
                    layouts_height,
                    query: &query,
                    search: &self.hostlist.search,
                },
                &t,
                cx,
            )
            .into_any_element()
        } else {
            div().into_any_element()
        };
        let form = self.render_form(window, cx);
        let import = self.render_import(window, cx);
        let snippet_picker = self.render_snippet_picker(window, cx);
        let snippet_form = self.render_snippet_form(window, cx);
        let layout_prompt = self.render_layout_prompt(window, cx);
        let group_prompt = self.render_group_prompt(window, cx);
        let panel_bg = self.panel_background();
        let hero = self.empty_view_hero(window);
        let layers = self.backdrop();
        let status_line = self.render_status_line(show_workspace, self.veil(panel_bg), cx);
        let theme_picker = self.render_theme_picker(window, cx);
        let toast = self.render_toast(window, cx);
        let context_menu = self.render_menu(cx);
        let collapsed = self.settings.sidebar_collapsed;
        let handle = (!collapsed && self.sidebar_tween.is_none()).then(|| self.resize_handle(cx));
        let root = div()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(|s, _, _, _| s.last_input = std::time::Instant::now()))
            .capture_any_mouse_down(
                cx.listener(|s, _, _, _| s.last_input = std::time::Instant::now()),
            )
            .on_mouse_move(cx.listener(|s, _, _, _| s.last_input = std::time::Instant::now()))
            .on_scroll_wheel(cx.listener(|s, _, _, _| s.last_input = std::time::Instant::now()))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(self.window_base())
            .children(layers)
            .font(crate::theme::ui_font())
            .text_color(t.text)
            .on_action(cx.listener(|s, _: &CloseTab, w, cx| s.close_pane_or_tab(w, cx)))
            .on_action(
                cx.listener(|s, _: &SplitRight, w, cx| s.split_pane(split::Axis::Row, w, cx)),
            )
            .on_action(
                cx.listener(|s, _: &SplitDown, w, cx| s.split_pane(split::Axis::Column, w, cx)),
            )
            .on_action(cx.listener(|s, _: &FocusPaneLeft, w, cx| {
                s.move_pane_focus(split::Direction::Left, w, cx)
            }))
            .on_action(cx.listener(|s, _: &FocusPaneRight, w, cx| {
                s.move_pane_focus(split::Direction::Right, w, cx)
            }))
            .on_action(cx.listener(|s, _: &FocusPaneUp, w, cx| {
                s.move_pane_focus(split::Direction::Up, w, cx)
            }))
            .on_action(cx.listener(|s, _: &FocusPaneDown, w, cx| {
                s.move_pane_focus(split::Direction::Down, w, cx)
            }))
            .on_action(cx.listener(|s, _: &NextTab, w, cx| {
                s.show_tab(tabs::step(s.active, s.tabs.len(), 1), w, cx)
            }))
            .on_action(cx.listener(|s, _: &PrevTab, w, cx| {
                s.show_tab(tabs::step(s.active, s.tabs.len(), -1), w, cx)
            }))
            .on_action(cx.listener(|s, a: &ActivateTab, w, cx| s.show_tab(a.0, w, cx)))
            .on_action(cx.listener(|s, _: &ToggleHostPicker, w, cx| s.toggle_picker(w, cx)))
            .on_action(cx.listener(|s, _: &FocusHostSearch, w, cx| s.focus_search(w, cx)))
            .on_action(cx.listener(|s, _: &ToggleSnippets, w, cx| s.toggle_snippet_picker(w, cx)))
            .on_action(cx.listener(|s, _: &ToggleSidebar, _, cx| s.toggle_sidebar(cx)))
            .on_action(cx.listener(|s, _: &ToggleSftp, w, cx| s.toggle_sftp(w, cx)))
            .on_action(cx.listener(|s, _: &FillPassword, _, cx| s.fill_password(cx)))
            .on_action(cx.listener(|s, _: &NewConnection, w, cx| s.open_form(None, None, w, cx)))
            .on_action(cx.listener(|s, _: &OpenSettings, w, cx| s.toggle_settings(w, cx)))
            .on_action(cx.listener(|s, _: &SaveLayout, w, cx| s.save_layout(w, cx)))
            // Capture phase: the terminal handles Escape itself, and ending a broadcast must
            // not depend on which pane has focus.
            .capture_key_down(cx.listener(|s, e: &gpui::KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape"
                    && e.keystroke.modifiers == gpui::Modifiers::default()
                    && s.on_broadcast_escape(cx)
                {
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(|s, e: &gpui::KeyDownEvent, w, cx| {
                if s.on_rename_key(e, w, cx)
                    || s.on_layout_prompt_key(e, w, cx)
                    || s.on_group_prompt_key(e, w, cx)
                {
                    cx.stop_propagation();
                    return;
                }
                if e.keystroke.key == "escape" && s.context_menu.is_some() {
                    s.context_menu = None;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if e.keystroke.key == "escape" && s.settings_page.is_some() && s.form.is_none() {
                    s.toggle_settings(w, cx);
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|s, _: &IncreaseFontSize, _, cx| {
                s.change_font(|st| st.step_font(1.0), cx)
            }))
            .on_action(cx.listener(|s, _: &DecreaseFontSize, _, cx| {
                s.change_font(|st| st.step_font(-1.0), cx)
            }))
            .on_action(cx.listener(|s, _: &ResetFontSize, _, cx| {
                s.change_font(|st| st.terminal_font_size = settings::FONT_DEFAULT, cx)
            }))
            .on_drag_move(
                cx.listener(|s, e: &gpui::DragMoveEvent<SidebarResize>, _, cx| {
                    s.on_sidebar_drag(f32::from(e.event.position.x), cx)
                }),
            )
            .on_drag_move(cx.listener(
                |s, e: &gpui::DragMoveEvent<sidebar::LayoutsResize>, window, cx| {
                    s.on_layouts_drag(
                        f32::from(e.event.position.y),
                        f32::from(window.viewport_size().height),
                        cx,
                    );
                },
            ))
            .child(chrome::render_titlebar(
                &t,
                window.is_fullscreen(),
                !self.settings.sidebar_collapsed,
                strip,
                self.tile_tint(t.shell),
                cx,
            ))
            .child(match self.settings_page {
                Some(section) => self.render_settings(section, window, cx),
                None => div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .relative()
                    .child(self.side_tile(sidebar_now, sidebar))
                    .child(
                        self.main_tile(panel_bg)
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .child(self.panel_content(hero, panel_bg, cx)),
                            )
                            .when_some(status_line, |el, line| el.child(line)),
                    )
                    .children(handle)
                    .into_any_element(),
            })
            .when_some(form, |el, form| el.child(form))
            .when_some(import, |el, i| el.child(i))
            .when_some(snippet_picker, |el, p| el.child(p))
            .when_some(snippet_form, |el, f| el.child(f))
            .when_some(layout_prompt, |el, p| el.child(p))
            .when_some(group_prompt, |el, prompt| el.child(prompt))
            .when_some(theme_picker, |el, p| el.child(p))
            .when_some(context_menu, |el, m| el.child(m))
            .when_some(self.render_broadcast_picker(cx), |el, p| el.child(p))
            .children(self.render_panels(window, cx))
            .when_some(toast, |el, t| el.child(t))
            .when_some(self.picker.as_ref(), |el, p| {
                let hits = self.picker_hits();
                let recent = self.recent_aliases();
                el.child(picker::render(
                    p,
                    &picker::Listed {
                        hosts: &self.hosts,
                        layouts: &self.layouts.items,
                        hits: &hits,
                        recent: &recent,
                    },
                    window.viewport_size(),
                    &t,
                    cx,
                ))
            });
        // Another frame while any hover fade is mid-flight (zeron drives it the same way).
        if crate::hover::fades_active() {
            window.request_animation_frame();
        }
        root
    }
}
