use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable, button::ButtonVariants, h_flex, v_flex,
};

use super::{
    ConnectionState, SettingsView, connection::EXPOSED_WARNING, group, page_header, row, row_with,
    traffic, value,
};
use crate::{
    app::stats::HostKeyPrompt,
    settings::{SettingsField, SettingsTab},
};

impl SettingsView {
    pub(super) fn general(&self, cx: &App) -> impl IntoElement {
        let app = self.app.read(cx);
        let stats = &app.stats_snapshot;
        let running = app.proxy.is_running();
        let state = ConnectionState::of(stats, running);
        let config = app.config().clone();
        let ready = config.validate_ready().is_ok();
        let server = if config.server.is_empty() {
            "Not set up".to_owned()
        } else {
            format!("{}@{}:{}", config.username, config.server, config.port)
        };
        let detail = if running {
            stats.status.clone()
        } else if ready {
            "Start the proxy to route traffic through your SSH server.".into()
        } else {
            "Add your SSH server details to get started.".into()
        };
        let action = if running {
            self.button("stop", "Stop", SettingsField::Stop)
                .icon(IconName::Pause)
        } else if ready {
            self.button("start", "Start", SettingsField::Start)
                .icon(IconName::Play)
                .primary()
        } else {
            self.button(
                "set-up",
                "Set Up…",
                SettingsField::Tab(SettingsTab::Connection),
            )
            .primary()
        };
        let color = state.color(cx);

        let status = h_flex()
            .gap_3()
            .p_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .size(px(36.))
                    .rounded_full()
                    .bg(color.opacity(0.15))
                    .child(Icon::new(state.icon()).size_5().text_color(color)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(state.label()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(detail),
                    ),
            )
            .child(action)
            .into_any_element();
        let error = stats.last_error.clone().map(|error| {
            h_flex()
                .items_start()
                .gap_2()
                .px_4()
                .py_2p5()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(Icon::new(IconName::TriangleAlert).size_4().mt_0p5())
                .child(div().flex_1().min_w_0().child(error))
                .into_any_element()
        });
        let overview = [status].into_iter().chain(error).chain([
            if config.local_addr.ip().is_loopback() {
                row("SOCKS5 proxy", value(config.local_addr.to_string(), cx), cx)
            } else {
                row_with(
                    "SOCKS5 proxy",
                    div().text_color(cx.theme().danger).child(EXPOSED_WARNING),
                    value(config.local_addr.to_string(), cx),
                    cx,
                )
            },
            row("SSH server", value(server, cx), cx),
            row("Traffic", value(traffic(stats), cx), cx),
        ]);

        div()
            .id("general-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .p_6()
                    .gap_5()
                    .child(page_header(
                        "General",
                        "Proxy status and what happens at startup.",
                        cx,
                    ))
                    .children(
                        stats
                            .host_key_prompt
                            .clone()
                            .map(|prompt| self.host_key_prompt(prompt, running, cx)),
                    )
                    .child(group(None, overview, cx))
                    .child(group(
                        Some("Startup"),
                        [
                            row_with(
                                "Open at login",
                                "Start ProxyBear in the menu bar when you log in.",
                                self.switch(
                                    "autostart",
                                    config.autostart,
                                    SettingsField::Autostart,
                                ),
                                cx,
                            ),
                            row_with(
                                "Connect on launch",
                                "Start the proxy as soon as ProxyBear opens.",
                                self.switch(
                                    "auto-connect",
                                    config.auto_connect,
                                    SettingsField::AutoConnect,
                                ),
                                cx,
                            ),
                        ],
                        cx,
                    ))
                    .child(group(
                        None,
                        [row_with(
                            "Settings file",
                            div().truncate().child(app.config_path.clone()),
                            self.button(
                                "reveal-config",
                                "Show in Finder",
                                SettingsField::RevealConfig,
                            )
                            .small()
                            .ghost(),
                            cx,
                        )],
                        cx,
                    )),
            )
    }

    fn host_key_prompt(&self, prompt: HostKeyPrompt, running: bool, cx: &App) -> impl IntoElement {
        let changed = prompt.previous.is_some();
        let color = if changed {
            cx.theme().danger
        } else {
            cx.theme().warning
        };
        let (title, body) = if changed {
            (
                "Server identity changed",
                format!(
                    "The host key of {} differs from the one you trusted. Servers get new keys when \
                     they are reinstalled, but this can also mean someone is intercepting the \
                     connection. Only trust the new key if you know why it changed.",
                    prompt.server
                ),
            )
        } else {
            (
                "Verify server identity",
                format!(
                    "This is the first connection to {}. Check that the fingerprint matches the \
                     server's key, for example with ssh-keygen -lf on the server's \
                     /etc/ssh/ssh_host_*_key.pub.",
                    prompt.server
                ),
            )
        };
        let trust_label = match (changed, running) {
            (true, _) => "Trust New Key",
            (false, true) => "Trust and Connect",
            (false, false) => "Trust",
        };

        v_flex()
            .gap_3()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(color)
            .bg(color.opacity(0.08))
            .child(
                h_flex()
                    .gap_2()
                    .text_color(color)
                    .child(Icon::new(IconName::TriangleAlert).size_4())
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    ),
            )
            .child(div().text_sm().child(body))
            .when_some(prompt.previous, |card, previous| {
                card.child(fingerprint("Trusted key", previous, cx))
            })
            .child(fingerprint(
                if changed { "New key" } else { "Fingerprint" },
                prompt.fingerprint,
                cx,
            ))
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        self.button(
                            "reject-host-key",
                            "Don't Trust",
                            SettingsField::RejectHostKey,
                        )
                        .ghost(),
                    )
                    .child(
                        self.button("trust-host-key", trust_label, SettingsField::TrustHostKey)
                            .when(changed, |button| button.danger())
                            .when(!changed, |button| button.primary()),
                    ),
            )
    }
}

fn fingerprint(label: &'static str, fingerprint: String, cx: &App) -> impl IntoElement {
    v_flex()
        .gap_0p5()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().font_family("Menlo").text_xs().child(fingerprint))
}
