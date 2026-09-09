use super::{SettingsField, SettingsView};
use gpui::{prelude::*, *};
use gpui_component::ActiveTheme;

impl SettingsView {
    pub(super) fn logs(&self, cx: &App) -> impl IntoElement {
        let app = self.app.read(cx);
        let logs = &app.log_tail;
        div()
            .flex()
            .flex_col()
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
                        view.child("No log entries yet. Connection activity will appear here.")
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
