use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex,
    input::Input,
    v_flex,
};

use super::{SettingsView, group, input, page_header, row, row_with};
use crate::{
    config::{AuthMethod, MAX_POOL_SIZE},
    settings::{SettingsField, SettingsForm},
};

pub(super) const EXPOSED_WARNING: &str =
    "Other devices can reach this address and use the proxy without a password.";

impl SettingsView {
    pub(super) fn connection(&self, cx: &App) -> impl IntoElement {
        let app = self.app.read(cx);
        let form = &app.form;
        let is_key = form.auth_method == AuthMethod::Key;
        let running = app.proxy.is_running();
        let (saved, host_key) = {
            let config = app.config();
            (
                SettingsForm::from_config(&config),
                config.host_fingerprint.clone(),
            )
        };
        let dirty = *form != saved;
        let save_error = form.save_error();
        let connection_error = form.connection_error();
        let can_start = save_error.is_none() && connection_error.is_none();
        let can_save = dirty && save_error.is_none();
        let message: Option<(SharedString, Hsla)> = app
            .stats_snapshot
            .last_error
            .clone()
            .map(|error| (error.into(), cx.theme().danger))
            .or_else(|| save_error.map(|error| (error.into(), cx.theme().warning)))
            .or_else(|| {
                app.feedback
                    .clone()
                    .map(|feedback| (feedback.into(), cx.theme().foreground))
            })
            .or_else(|| connection_error.map(|error| (error.into(), cx.theme().warning)))
            .or_else(|| {
                dirty.then(|| {
                    let text = if running {
                        "Unsaved changes. Restart the proxy to apply them."
                    } else {
                        "Unsaved changes."
                    };
                    (text.into(), cx.theme().muted_foreground)
                })
            });

        let server = group(
            Some("SSH server"),
            [
                row("Hostname", input(&self.server, 260.), cx),
                row("Port", input(&self.port, 80.), cx),
                row("Username", input(&self.username, 260.), cx),
                row_with(
                    "Host key",
                    div().truncate().child(host_key.clone().unwrap_or_else(|| {
                        "Not verified yet. You will be asked on the next connection.".into()
                    })),
                    self.button("forget-host-key", "Forget", SettingsField::ForgetHostKey)
                        .small()
                        .danger()
                        .ghost()
                        .disabled(host_key.is_none()),
                    cx,
                ),
            ],
            cx,
        );

        let method = row(
            "Sign in with",
            ButtonGroup::new("auth-method")
                .child(self.choice_button(
                    "auth-key",
                    "Private key",
                    SettingsField::AuthMethod(AuthMethod::Key),
                    is_key,
                ))
                .child(self.choice_button(
                    "auth-password",
                    "Password",
                    SettingsField::AuthMethod(AuthMethod::Password),
                    !is_key,
                )),
            cx,
        );
        let credentials = if is_key {
            vec![
                row(
                    "Private key",
                    h_flex().gap_2().child(input(&self.key_path, 222.)).child(
                        Button::new("choose-key")
                            .icon(IconName::FolderOpen)
                            .small()
                            .tooltip("Choose a key file")
                            .on_click(self.on_field(SettingsField::ChooseKey)),
                    ),
                    cx,
                ),
                row_with(
                    "Passphrase",
                    "Leave empty if the key is not encrypted. Stored in your Keychain.",
                    masked(&self.key_password),
                    cx,
                ),
            ]
        } else {
            vec![row_with(
                "Password",
                "Stored in your Keychain.",
                masked(&self.ssh_password),
                cx,
            )]
        };
        let auth = group(
            Some("Authentication"),
            [method].into_iter().chain(credentials),
            cx,
        );

        let proxy = group(
            Some("Local proxy"),
            [
                row_with(
                    "SOCKS5 address",
                    if form.exposes_proxy() {
                        div().text_color(cx.theme().danger).child(EXPOSED_WARNING)
                    } else {
                        div().child("No authentication. Keep it on a loopback address.")
                    },
                    input(&self.local_addr, 180.),
                    cx,
                ),
                row_with(
                    "SSH sessions",
                    format!(
                        "Parallel SSH connections, 1 to {MAX_POOL_SIZE}. New connections use the least busy one."
                    ),
                    input(&self.pool_size, 64.),
                    cx,
                ),
            ],
            cx,
        );

        v_flex()
            .size_full()
            .child(
                div()
                    .id("connection-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .p_6()
                            .gap_5()
                            .child(page_header(
                                "Connection",
                                "The SSH server ProxyBear tunnels through, and the local proxy it serves.",
                                cx,
                            ))
                            .child(server)
                            .child(auth)
                            .child(proxy),
                    ),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_2()
                    .px_6()
                    .py_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .when_some(message, |line, (text, color)| {
                                line.text_color(color).child(text)
                            }),
                    )
                    .when(dirty, |footer| {
                        footer.child(
                            self.button("revert", "Revert", SettingsField::Revert)
                                .ghost(),
                        )
                    })
                    .child(self.button("save", "Save", SettingsField::Save).disabled(!can_save))
                    .child(
                        self.button(
                            "start",
                            if running {
                                "Save and Restart"
                            } else {
                                "Save and Start"
                            },
                            SettingsField::SaveAndStart,
                        )
                        .primary()
                        .disabled(!can_start),
                    ),
            )
    }
}

fn masked(state: &Entity<gpui_component::input::InputState>) -> impl IntoElement {
    div()
        .w(px(260.))
        .child(Input::new(state).small().mask_toggle())
}
