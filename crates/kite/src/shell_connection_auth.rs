//! The connection form's Authentication section: SSH key vs Password, file picker, vault key
//! chips, and inline vault unlock/create step.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, px,
};
use kite_vault::Key;

use super::{AuthMode, ConnectionForm, VaultStep, button, note, row};
use crate::hover::HoverFade as _;
use crate::i18n::tr;
use crate::keeper::Keeper;
use crate::settings_widgets::toggle;
use crate::shell::Shell;

impl Shell {
    pub(super) fn pick_connection_key_file(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("Choose SSH private key").into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let text = path.to_string_lossy().into_owned();
            let _ = this.update(cx, |shell, cx| {
                if let Some(form) = shell.form.as_ref() {
                    let key_input = form.key.clone();
                    key_input.update(cx, |i, cx| i.set_text(text, cx));
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn auth_section(
        &self,
        form: &ConnectionForm,
        step: VaultStep,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = &self.theme;
        let auth_tabs = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_xs()
                    .text_color(t.muted)
                    .child(tr("Authentication")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        button(
                            "auth-mode-key",
                            tr("SSH key"),
                            form.auth_mode == AuthMode::Key,
                            t,
                        )
                        .on_click(cx.listener(|shell, _, _, cx| {
                            if let Some(form) = shell.form.as_mut() {
                                form.auth_mode = AuthMode::Key;
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        button(
                            "auth-mode-password",
                            tr("Password"),
                            form.auth_mode == AuthMode::Password,
                            t,
                        )
                        .on_click(cx.listener(|shell, _, _, cx| {
                            if let Some(form) = shell.form.as_mut() {
                                form.auth_mode = AuthMode::Password;
                            }
                            cx.notify();
                        })),
                    ),
            );
        let mut section = div().flex().flex_col().gap(px(10.)).child(auth_tabs);
        match form.auth_mode {
            AuthMode::Key => {
                let vault_ssh_keys: Vec<String> = Keeper::keys(cx)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|k| match k {
                        Key::SshKey { name } => Some(name),
                        _ => None,
                    })
                    .collect();
                let vault_key_chips = div().flex().flex_wrap().gap(px(6.)).children(
                    vault_ssh_keys.into_iter().enumerate().map(|(n, name)| {
                        let pick = name.clone();
                        div()
                            .id(("vault-key-chip", n))
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(px(6.))
                            .text_xs()
                            .text_color(t.muted)
                            .hover_fade(format!("vault-key-chip-{n}"), t.row_hover, t.row_active)
                            .cursor_pointer()
                            .on_click(cx.listener(move |shell, _, _, cx| {
                                if let Some(form) = shell.form.as_ref() {
                                    let vk = form.vault_key.clone();
                                    vk.update(cx, |i, cx| i.set_text(pick.clone(), cx));
                                }
                            }))
                            .child(SharedString::from(name))
                    }),
                );
                let key_file_row = div()
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .child(div().flex_1().child(row(tr("Key file"), &form.key, t)))
                    .child(
                        button("form-browse-key", tr("Browse…"), false, t).on_click(
                            cx.listener(|shell, _, _, cx| shell.pick_connection_key_file(cx)),
                        ),
                    );
                section = section
                    .child(key_file_row)
                    .child(row(tr("Vault key"), &form.vault_key, t))
                    .child(vault_key_chips);
            }
            AuthMode::Password => {
                section = section.child(row(tr("Password"), &form.password, t));
                match step {
                    VaultStep::None => {}
                    VaultStep::Unlock => {
                        section = section.child(row(tr("Vault passphrase"), &form.vault_pass, t));
                    }
                    VaultStep::Create => {
                        section = section
                            .child(note(
                                tr("No vault yet: this creates one, locked with this passphrase."),
                                t,
                            ))
                            .child(row(tr("New vault passphrase"), &form.vault_pass, t))
                            .child(row(tr("Repeat passphrase"), &form.vault_repeat, t))
                            .child(
                                div()
                                    .id("form-remember-keychain")
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .py(px(2.))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|shell, _, _, cx| {
                                        if let Some(form) = shell.form.as_mut() {
                                            form.remember_in_keychain = !form.remember_in_keychain;
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(t.muted)
                                            .child(tr("Remember vault passphrase in Keychain")),
                                    )
                                    .child(toggle(
                                        t,
                                        form.remember_in_keychain,
                                        "form-remember-keychain-toggle",
                                    )),
                            );
                    }
                }
            }
        }
        section
    }
}
