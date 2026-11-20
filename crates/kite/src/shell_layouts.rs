//! Saving, opening and renaming workspace layouts.

use gpui::{
    AnyElement, AppContext, Context, Entity, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, px,
};

use super::connections_ui::{button, row};
use super::{Shell, ToastKind};
use crate::i18n::{fill, tr};
use crate::layouts::{self, Load, SavedLayouts};
use crate::text_input::{InputColors, TextInput};

pub(super) struct State {
    pub items: Vec<layouts::SavedLayout>,
    pub unreadable: bool,
    pub prompt: Option<Prompt>,
}

pub(super) struct Prompt {
    input: Entity<TextInput>,
    kind: Kind,
}

enum Kind {
    SaveNew { tab: usize },
    Rename(String),
}

impl State {
    pub(super) fn load() -> Self {
        let Some(dir) = crate::settings::dir() else {
            return Self::blank(false);
        };
        match SavedLayouts::load(&dir) {
            Load::Ready(saved) => Self {
                items: saved.layouts,
                unreadable: false,
                prompt: None,
            },
            Load::Missing => Self::blank(false),
            Load::Unreadable => Self::blank(true),
        }
    }

    fn blank(unreadable: bool) -> Self {
        Self {
            items: Vec::new(),
            unreadable,
            prompt: None,
        }
    }
}

impl Shell {
    /// After a sync pull, replace the in-memory layouts with the file on disk.
    pub(super) fn reload_layouts(&mut self) {
        let Some(dir) = crate::settings::dir() else {
            return;
        };
        match SavedLayouts::load(&dir) {
            Load::Missing => {
                self.layouts.items.clear();
                self.layouts.unreadable = false;
            }
            Load::Ready(saved) => {
                self.layouts.items = saved.layouts;
                self.layouts.unreadable = false;
            }
            Load::Unreadable => self.layouts.unreadable = true,
        }
    }

    /// Cmd+S: update the linked layout, or ask for a name when the tab has none.
    pub(crate) fn save_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.layouts.prompt.is_some() {
            self.commit_layout_prompt(window, cx);
            return;
        }
        let tab = self.active;
        let linked = self.tabs.get(tab).and_then(|tab| tab.layout_id.clone());
        match linked {
            Some(id) if self.layouts.items.iter().any(|layout| layout.id == id) => {
                self.update_layout(tab, &id, cx);
            }
            _ => self.prompt_save_new(tab, window, cx),
        }
    }

    /// Asks for a name, then saves tab `tab` as a new layout and links the tab to it.
    pub(crate) fn prompt_save_new(
        &mut self,
        tab: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(label) = self.tabs.get(tab).map(|tab| tab.strip_label()) else {
            self.notify_toast(ToastKind::Critical, tr("No session to save"), cx);
            return;
        };
        self.open_prompt(Kind::SaveNew { tab }, &label, window, cx);
    }

    pub(crate) fn prompt_rename_layout(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = self
            .layouts
            .items
            .iter()
            .find(|layout| layout.id == id)
            .map(|layout| layout.name.clone())
        else {
            return;
        };
        self.open_prompt(Kind::Rename(id.to_owned()), &name, window, cx);
    }

    /// Enter saves and Escape drops the name field.
    pub(super) fn on_layout_prompt_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.layouts.prompt.is_none() {
            return false;
        }
        match event.keystroke.key.as_str() {
            "enter" => self.commit_layout_prompt(window, cx),
            "escape" => self.cancel_layout_prompt(window, cx),
            _ => return false,
        }
        true
    }

    fn open_prompt(&mut self, kind: Kind, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let t = self.theme;
        let colors = InputColors {
            text: t.text,
            placeholder: t.faint,
            cursor: t.accent,
            selection: t.accent.opacity(0.35),
        };
        let input = cx.new(|cx| {
            let mut input = TextInput::new(tr("Layout name"), false, colors, cx);
            input.set_text(text, cx);
            input.select_everything(cx);
            input
        });
        window.focus(&gpui::Focusable::focus_handle(input.read(cx), cx), cx);
        self.layouts.prompt = Some(Prompt { input, kind });
        cx.notify();
    }

    fn commit_layout_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.layouts.prompt.take() else {
            return;
        };
        let name = prompt.input.read(cx).text().trim().to_owned();
        if name.is_empty() {
            self.layouts.prompt = Some(prompt);
            self.notify_toast(ToastKind::Critical, tr("Name is required."), cx);
            return;
        }
        let saved = match &prompt.kind {
            Kind::SaveNew { tab } => self.create_layout(*tab, &name, cx),
            Kind::Rename(id) => self.rename_layout(id, &name, cx),
        };
        if let Err(error) = saved {
            self.layouts.prompt = Some(prompt);
            self.notify_toast(ToastKind::Critical, error, cx);
            return;
        }
        self.restore_focus(window, cx);
    }

    fn cancel_layout_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.layouts.prompt.take().is_some() {
            self.restore_focus(window, cx);
        }
    }

    fn create_layout(
        &mut self,
        tab: usize,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some((tree, focused, title)) = self.captured_layout(tab) else {
            return Err(tr("No session to save").into());
        };
        let mut book = SavedLayouts {
            version: layouts::CURRENT,
            layouts: self.layouts.items.clone(),
        };
        let Some(id) = book.insert_new(name, tree, focused, title) else {
            return Err(tr("No session to save").into());
        };
        self.store_layouts(book.layouts)?;
        if let Some(tab) = self.tabs.get_mut(tab) {
            tab.layout_id = Some(id);
        }
        self.persist_tabs();
        self.notify_toast(ToastKind::Positive, fill("Saved layout “{}”", &[name]), cx);
        Ok(())
    }

    pub(crate) fn update_layout(&mut self, tab: usize, id: &str, cx: &mut Context<Self>) {
        let Some(name) = self
            .layouts
            .items
            .iter()
            .find(|layout| layout.id == id)
            .map(|layout| layout.name.clone())
        else {
            return;
        };
        let Some((tree, focused, title)) = self.captured_layout(tab) else {
            self.notify_toast(ToastKind::Critical, tr("No session to save"), cx);
            return;
        };
        let mut book = SavedLayouts {
            version: layouts::CURRENT,
            layouts: self.layouts.items.clone(),
        };
        if !book.update(id, tree, focused, title) {
            self.notify_toast(ToastKind::Critical, tr("No session to save"), cx);
            return;
        }
        if let Err(error) = self.store_layouts(book.layouts) {
            tracing::warn!(error = %error, "layouts_save_failed");
            self.notify_toast(ToastKind::Critical, error, cx);
            return;
        }
        self.notify_toast(
            ToastKind::Positive,
            fill("Updated layout “{}”", &[&name]),
            cx,
        );
    }

    fn rename_layout(
        &mut self,
        id: &str,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut book = SavedLayouts {
            version: layouts::CURRENT,
            layouts: self.layouts.items.clone(),
        };
        if !book.rename(id, name) {
            return Ok(());
        }
        self.store_layouts(book.layouts)?;
        self.notify_toast(
            ToastKind::Positive,
            fill("Renamed layout to “{}”", &[name]),
            cx,
        );
        Ok(())
    }

    pub(crate) fn remove_layout(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(name) = self
            .layouts
            .items
            .iter()
            .find(|layout| layout.id == id)
            .map(|layout| layout.name.clone())
        else {
            return;
        };
        let mut book = SavedLayouts {
            version: layouts::CURRENT,
            layouts: self.layouts.items.clone(),
        };
        if !book.remove(id) {
            return;
        }
        if let Err(error) = self.store_layouts(book.layouts) {
            tracing::warn!(error = %error, "layouts_save_failed");
            self.notify_toast(ToastKind::Critical, error, cx);
            return;
        }
        let mut changed = false;
        for tab in &mut self.tabs {
            if tab.layout_id.as_deref() == Some(id) {
                tab.layout_id = None;
                changed = true;
            }
        }
        if changed {
            self.persist_tabs();
        }
        self.notify_toast(
            ToastKind::Default,
            fill("Removed layout “{}”", &[&name]),
            cx,
        );
        cx.notify();
    }

    /// Opens `id` in a new tab and connects every pane that is still valid.
    pub(crate) fn open_layout_id(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = self
            .layouts
            .items
            .iter()
            .find(|item| item.id == id)
            .cloned()
        else {
            return;
        };
        let opened = {
            let known = |alias: &str| self.hosts.iter().any(|host| host.alias == alias);
            layouts::prune(&layout.tree, layout.focused, |alias| {
                layouts::alias_openable(alias, known)
            })
        };
        let Ok(opened) = opened else {
            self.notify_toast(
                ToastKind::Critical,
                fill("Could not open “{}”", &[&layout.name]),
                cx,
            );
            return;
        };
        if !opened.missing.is_empty() {
            let names = opened.missing.join(", ");
            self.notify_toast(
                ToastKind::Default,
                fill("Skipped missing hosts: {}", &[&names]),
                cx,
            );
        }
        if !self.spawn_tree(
            super::tabs_ui::Spawn {
                node: &opened.tree,
                focused: opened.focused,
                title: layout.title.clone(),
                layout_id: Some(layout.id),
                dial: true,
            },
            window,
            cx,
        ) {
            self.notify_toast(
                ToastKind::Critical,
                fill("Could not open “{}”", &[&layout.name]),
                cx,
            );
            return;
        }
        let ix = self.tabs.len().saturating_sub(1);
        self.show_tab(ix, window, cx);
    }

    pub(crate) fn open_layout_at(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.layouts.items.get(ix).map(|layout| layout.id.clone()) else {
            return;
        };
        self.open_layout_id(&id, window, cx);
    }

    fn store_layouts(&mut self, next: Vec<layouts::SavedLayout>) -> Result<(), String> {
        if self.layouts.unreadable {
            return Err(tr("layouts.json could not be read; it was not overwritten").into());
        }
        let book = SavedLayouts {
            version: layouts::CURRENT,
            layouts: next,
        };
        if let Some(dir) = crate::settings::dir() {
            book.save(&dir).map_err(|error| {
                tracing::warn!(error = %error, "layouts_save_failed");
                error.to_string()
            })?;
        }
        self.layouts.items = book.layouts;
        Ok(())
    }

    pub(super) fn render_layout_prompt(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let prompt = self.layouts.prompt.as_ref()?;
        let t = &self.theme;
        let viewport = window.viewport_size();
        let title = match prompt.kind {
            Kind::SaveNew { .. } => tr("Save layout"),
            Kind::Rename(_) => tr("Rename layout"),
        };
        let card = div()
            .id("layout-prompt")
            .w(px(360.0_f32.min(f32::from(viewport.width) - 32.0)))
            .flex()
            .flex_col()
            .rounded(px(16.))
            .border_1()
            .border_color(t.border)
            .bg(t.popup)
            .text_color(t.text)
            .on_mouse_down_out(cx.listener(|shell, _, window, cx| {
                shell.cancel_layout_prompt(window, cx);
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
                    .child(row(tr("Layout name"), &prompt.input, t)),
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
                    .child(button("layout-cancel", tr("Cancel"), false, t).on_click(
                        cx.listener(|shell, _, window, cx| shell.cancel_layout_prompt(window, cx)),
                    ))
                    .child(button("layout-save", tr("Save"), true, t).on_click(
                        cx.listener(|shell, _, window, cx| shell.commit_layout_prompt(window, cx)),
                    )),
            );
        Some(layout_overlay(card.into_any_element(), viewport, t))
    }
}

fn layout_overlay(
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
