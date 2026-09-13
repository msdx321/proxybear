mod logs;
mod settings_tab;

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    input::{InputEvent, InputState},
    v_flex,
};

use super::{SettingsField, SettingsTab};
use crate::ProxyBear;

pub struct SettingsView {
    app: Entity<ProxyBear>,
    server: Entity<InputState>,
    username: Entity<InputState>,
    port: Entity<InputState>,
    key_path: Entity<InputState>,
    key_password: Entity<InputState>,
    ssh_password: Entity<InputState>,
    local_addr: Entity<InputState>,
    log_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(app: Entity<ProxyBear>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let form = app.read(cx).form.clone();
        let mut subscriptions = vec![cx.observe(&app, |_, _, cx| cx.notify())];
        let mut input = |value: String,
                         placeholder: &'static str,
                         masked: bool,
                         field: fn(String) -> SettingsField| {
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value)
                    .placeholder(placeholder)
                    .masked(masked)
            });
            let app = app.clone();
            subscriptions.push(cx.subscribe(&state, move |_, state, event, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = state.read(cx).value().to_string();
                    app.update(cx, |app, cx| app.handle_field(field(value), cx));
                }
            }));
            state
        };
        Self {
            server: input(
                form.server,
                "host.example.com",
                false,
                SettingsField::Server,
            ),
            username: input(
                form.username,
                "SSH username",
                false,
                SettingsField::Username,
            ),
            port: input(form.port, "22", false, SettingsField::Port),
            key_path: input(
                form.key_path,
                "/Users/me/.ssh/id_ed25519",
                false,
                SettingsField::KeyPath,
            ),
            key_password: input(
                form.key_password,
                "Optional",
                true,
                SettingsField::KeyPassword,
            ),
            ssh_password: input(
                form.ssh_password,
                "SSH password",
                true,
                SettingsField::SshPassword,
            ),
            local_addr: input(
                form.local_addr,
                "127.0.0.1:1080",
                false,
                SettingsField::LocalAddr,
            ),
            app,
            log_scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    fn choice_button(
        &self,
        id: &'static str,
        label: &'static str,
        field: SettingsField,
        active: bool,
        cx: &App,
    ) -> Button {
        self.button(id, label, field).when_else(
            active,
            |button| {
                button.custom(
                    ButtonCustomVariant::new(cx)
                        .color(cx.theme().primary)
                        .foreground(cx.theme().primary_foreground)
                        .hover(cx.theme().primary_hover)
                        .active(cx.theme().primary_active)
                        .shadow(true),
                )
            },
            |button| button.ghost(),
        )
    }

    fn button(&self, id: &'static str, label: &'static str, field: SettingsField) -> Button {
        let app = self.app.clone();
        let key_path = self.key_path.clone();
        let scroll = self.log_scroll.clone();
        Button::new(id).label(label).on_click(move |_, window, cx| {
            let choose_key = matches!(field, SettingsField::ChooseKey);
            let reset_logs = matches!(
                field,
                SettingsField::Tab(SettingsTab::Logs) | SettingsField::ClearLog
            );
            app.update(cx, |app, cx| app.handle_field(field.clone(), cx));
            if choose_key {
                let value = app.read(cx).form.key_path.clone();
                key_path.update(cx, |input, cx| input.set_value(value, window, cx));
            }
            if reset_logs {
                scroll.set_offset(point(px(0.), px(0.)));
            }
        })
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app = self.app.read(cx);
        let active = app.active_tab;
        let stats = app.stats.snapshot();
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_6()
                    .py_4()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("ProxyBear"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Manage your SSH SOCKS5 proxy."),
                            ),
                    )
                    .child(
                        div()
                            .rounded_full()
                            .px_3()
                            .py_1()
                            .text_sm()
                            .bg(cx.theme().secondary)
                            .text_color(cx.theme().muted_foreground)
                            .when(stats.ssh_connected, |badge| {
                                badge
                                    .bg(cx.theme().success.opacity(0.15))
                                    .text_color(cx.theme().success)
                            })
                            .when(stats.last_error.is_some(), |badge| {
                                badge
                                    .bg(cx.theme().danger.opacity(0.15))
                                    .text_color(cx.theme().danger)
                            })
                            .child(stats.status.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_flex()
                            .w(px(156.))
                            .flex_shrink_0()
                            .p_3()
                            .gap_2()
                            .bg(cx.theme().muted)
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .child(self.choice_button(
                                "settings",
                                "Connection",
                                SettingsField::Tab(SettingsTab::Settings),
                                active == SettingsTab::Settings,
                                cx,
                            ))
                            .child(self.choice_button(
                                "logs",
                                "Activity logs",
                                SettingsField::Tab(SettingsTab::Logs),
                                active == SettingsTab::Logs,
                                cx,
                            )),
                    )
                    .child(v_flex().flex_1().min_w_0().min_h_0().child(match active {
                        SettingsTab::Settings => self.settings(cx).into_any_element(),
                        SettingsTab::Logs => self.logs(cx).into_any_element(),
                    })),
            )
            .child(
                div()
                    .px_6()
                    .py_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(app.stats_text.clone()),
            )
    }
}
