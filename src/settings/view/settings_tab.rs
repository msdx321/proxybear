use super::{SettingsField, SettingsView};
use crate::config::AuthMethod;
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable,
    button::ButtonVariants,
    input::{Input, InputState},
};

impl SettingsView {
    pub(super) fn settings(&self, cx: &App) -> impl IntoElement {
        let app = self.app.read(cx);
        let form = &app.form;
        let is_key = form.auth_method != AuthMethod::Password.as_str();
        let running = app.proxy.is_running();
        let validation = form.save_error().or_else(|| form.start_error());
        let error = app.stats.snapshot().last_error;
        let auth = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.choice_button(
                        "auth-key",
                        "Private key",
                        SettingsField::AuthMethod("key".into()),
                        is_key,
                        cx,
                    ))
                    .child(self.choice_button(
                        "auth-password",
                        "Password",
                        SettingsField::AuthMethod("password".into()),
                        !is_key,
                        cx,
                    )),
            )
            .when(is_key, |auth| {
                auth.child(
                    div()
                        .flex()
                        .items_end()
                        .gap_2()
                        .child(field("Private key file", &self.key_path))
                        .child(self.button("choose-key", "Browse…", SettingsField::ChooseKey)),
                )
                .child(field("Key passphrase", &self.key_password))
            })
            .when(!is_key, |auth| {
                auth.child(field("SSH password", &self.ssh_password))
            });
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_5()
                    .gap_4()
                    .flex()
                    .flex_col()
                    .child(
                        panel(
                            "SSH server",
                            "The remote server that carries your traffic.",
                            cx,
                        )
                        .child(field("Hostname", &self.server))
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(field("Username", &self.username))
                                .child(
                                    div()
                                        .w(px(96.))
                                        .flex_shrink_0()
                                        .child(field("Port", &self.port)),
                                ),
                        ),
                    )
                    .child(
                        panel(
                            "Authentication",
                            "Choose how to sign in to your server.",
                            cx,
                        )
                        .child(auth),
                    )
                    .child(
                        panel(
                            "Local SOCKS5 proxy",
                            "Use this address in your browser or other apps.",
                            cx,
                        )
                        .child(field("Bind address", &self.local_addr)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .when_some(error, |footer, error| {
                        footer.child(div().text_sm().text_color(cx.theme().danger).child(error))
                    })
                    .when_some(validation, |footer, error| {
                        footer.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(error),
                        )
                    })
                    .when_some(app.feedback.clone(), |footer, feedback| {
                        footer.child(div().text_sm().child(feedback))
                    })
                    .when(running, |footer| {
                        footer.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Stop the proxy before starting with new settings."),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                self.button("save", "Save", SettingsField::Save)
                                    .disabled(!form.can_save()),
                            )
                            .child(
                                self.button("start", "Save and Start", SettingsField::SaveAndStart)
                                    .primary()
                                    .disabled(!form.can_start() || running),
                            )
                            .child(
                                self.button("stop", "Stop", SettingsField::Stop)
                                    .disabled(!running),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .overflow_hidden()
                            .child(app.config_path.clone()),
                    ),
            )
    }
}

fn field(label: &'static str, input: &Entity<InputState>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap_1()
        .child(div().text_sm().child(label))
        .child(Input::new(input))
}

fn panel(title: &'static str, description: &'static str, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .gap_3()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                ),
        )
}
