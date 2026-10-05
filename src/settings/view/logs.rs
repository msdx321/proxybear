use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, IconName, Sizable,
    button::{ButtonGroup, ButtonVariants},
    h_flex, v_flex,
};

use super::{SettingsView, group, page_header, row_with};
use crate::{config::LogLevel, settings::SettingsField};

impl SettingsView {
    pub(super) fn logs(&self, cx: &App) -> impl IntoElement {
        let app = self.app.read(cx);
        let logs = &app.log_tail;
        let log_level = app.config().log_level;
        let error = logs
            .error()
            .map(str::to_owned)
            .or_else(|| app.stats_snapshot.last_error.clone());
        let levels = ButtonGroup::new("log-level").children(
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
                )
            }),
        );

        v_flex()
            .size_full()
            .p_6()
            .gap_5()
            .child(page_header(
                "Logs",
                format!("{} · newest first", logs.status()),
                cx,
            ))
            .child(group(
                None,
                [row_with(
                    "Log level",
                    "Saved automatically. Applies to new entries.",
                    levels,
                    cx,
                )],
                cx,
            ))
            .when_some(app.feedback.clone(), |view, feedback| {
                view.child(div().text_sm().child(feedback))
            })
            .when_some(error, |view, error| {
                view.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(logs.path_label().to_owned()),
                            )
                            .child(
                                self.button("open-log", "Open", SettingsField::OpenLog)
                                    .icon(IconName::ExternalLink)
                                    .small()
                                    .ghost(),
                            )
                            .child(
                                self.button(
                                    "reveal-log",
                                    "Show in Finder",
                                    SettingsField::RevealLog,
                                )
                                .icon(IconName::FolderOpen)
                                .small()
                                .ghost(),
                            )
                            .child(
                                self.button("clear-log", "Clear", SettingsField::ClearLog)
                                    .icon(IconName::Delete)
                                    .small()
                                    .danger()
                                    .ghost(),
                            ),
                    )
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
                            .bg(cx.theme().secondary)
                            .font_family("Menlo")
                            .text_xs()
                            .when(logs.lines().is_empty(), |view| {
                                view.text_color(cx.theme().muted_foreground)
                                    .child("No log entries yet at the selected level.")
                            })
                            .children(
                                logs.lines()
                                    .iter()
                                    .rev()
                                    .map(|line| div().pb_1().child(line.clone())),
                            ),
                    ),
            )
    }
}
