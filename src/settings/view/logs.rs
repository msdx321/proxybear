use super::{SettingsField, SettingsView};
use crate::config::LogLevel;
use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, v_flex};

impl SettingsView {
    pub(super) fn logs(&self, cx: &App) -> impl IntoElement {
        let app = self.app.read(cx);
        let logs = &app.log_tail;
        let log_level = app.config_snapshot().log_level;
        v_flex()
            .size_full()
            .p_5()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Activity logs"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} · newest first", logs.status())),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_sm().child("Log level"))
                    .child(
                        div().flex().flex_wrap().gap_2().children(
                            [
                                (LogLevel::Error, "Error"),
                                (LogLevel::Warn, "Warn"),
                                (LogLevel::Info, "Info"),
                                (LogLevel::Debug, "Debug"),
                                (LogLevel::Trace, "Trace"),
                            ]
                            .into_iter()
                            .map(|(level, label)| {
                                self.choice_button(
                                    label,
                                    label,
                                    SettingsField::LogLevel(level),
                                    log_level == level,
                                    cx,
                                )
                            }),
                        ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Saved automatically. Applies to new log entries."),
                    ),
            )
            .when_some(app.feedback.clone(), |view, feedback| {
                view.child(div().text_sm().child(feedback))
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.button("open-log", "Open file", SettingsField::OpenLog))
                    .child(self.button("reveal-log", "Show in Finder", SettingsField::RevealLog))
                    .child(self.button("clear-log", "Clear", SettingsField::ClearLog)),
            )
            .when_some(logs.error(), |view, error| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error.to_owned()),
                )
            })
            .child(
                div()
                    .id("logs-scroll")
                    .track_scroll(&self.log_scroll)
                    .overflow_y_scroll()
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .font_family("Menlo")
                    .text_xs()
                    .when(logs.lines().is_empty(), |view| {
                        view.child("No log entries yet at the selected level.")
                    })
                    .children(
                        logs.lines()
                            .iter()
                            .rev()
                            .map(|line| div().pb_2().child(line.clone())),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(logs.path_label().to_owned()),
            )
    }
}
