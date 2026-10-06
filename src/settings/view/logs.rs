use gpui::{prelude::*, *};
use gpui_base::SelectableText;
use gpui_component::{
    ActiveTheme, Icon, IconName, Selectable, Sizable,
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex,
    input::Input,
    v_flex,
};

use super::{SettingsView, group, page_header, row_with};
use crate::{config::LogLevel, settings::SettingsField};

/// Which recorded lines the viewer shows. Separate from the log level,
/// which decides what gets recorded.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(super) enum LineFilter {
    #[default]
    All,
    Warnings,
    Errors,
}

impl LineFilter {
    fn shows(self, line: &str) -> bool {
        // Lines without a level, such as continuations, are always shown.
        let less_severe: &[&str] = match self {
            Self::All => &[],
            Self::Warnings => &["INFO", "DEBUG", "TRACE"],
            Self::Errors => &["WARN", "INFO", "DEBUG", "TRACE"],
        };
        line_level(line).is_none_or(|level| !less_severe.contains(&level))
    }
}

/// The level column of a `tracing` line: `<timestamp> <LEVEL> <target>: ...`.
fn line_level(line: &str) -> Option<&str> {
    line.split_whitespace().nth(1)
}

impl SettingsView {
    /// Lines that pass the filter and search, oldest first.
    fn visible_log_lines(&self, cx: &App) -> Vec<SharedString> {
        let query = self.log_query.read(cx).value().trim().to_lowercase();
        self.app
            .read(cx)
            .log_tail
            .lines()
            .iter()
            .filter(|line| {
                self.log_filter.shows(line)
                    && (query.is_empty() || line.to_lowercase().contains(&query))
            })
            .cloned()
            .collect()
    }

    fn copy_log_lines(&self, cx: &mut Context<Self>) {
        let lines = self.visible_log_lines(cx);
        cx.write_to_clipboard(ClipboardItem::new_string(lines.join("\n")));
        self.app.update(cx, |app, cx| {
            app.feedback = Some(format!("Copied {} lines.", lines.len()));
            cx.notify();
        });
    }

    pub(super) fn logs(&self, cx: &App) -> impl IntoElement {
        let app = self.app.read(cx);
        let logs = &app.log_tail;
        let log_level = app.config().log_level;
        let error = logs
            .error()
            .map(str::to_owned)
            .or_else(|| app.stats_snapshot.last_error.clone());
        let lines = self.visible_log_lines(cx);
        let log_focus = self.log_focus.clone();
        let empty = if logs.lines().is_empty() {
            "No log entries yet at the selected level."
        } else {
            "No entries match."
        };
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
        let filters = ButtonGroup::new("line-filter").children(
            [
                ("show-all", "All", LineFilter::All),
                ("show-warnings", "Warnings", LineFilter::Warnings),
                ("show-errors", "Errors", LineFilter::Errors),
            ]
            .into_iter()
            .map(|(id, label, filter)| {
                let active = self.log_filter == filter;
                Button::new(id)
                    .label(label)
                    .small()
                    .selected(active)
                    .when(active, |button| button.primary())
                    .on_click(self.on_view(move |view, _| view.log_filter = filter))
            }),
        );
        let action = |id, icon, tooltip, field| {
            Button::new(id)
                .icon(icon)
                .small()
                .ghost()
                .tooltip(tooltip)
                .on_click(self.on_field(field))
        };

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header(
                "Logs",
                format!("{} · newest first", logs.status()),
                cx,
            ))
            .child(group(
                None,
                [row_with("Log level", "Saved automatically.", levels, cx)],
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
                            .gap_2()
                            .child(
                                div().flex_1().min_w_0().child(
                                    Input::new(&self.log_query)
                                        .small()
                                        .prefix(Icon::new(IconName::Search).small())
                                        .cleanable(true),
                                ),
                            )
                            .child(filters),
                    )
                    .child(
                        div()
                            .id("logs-scroll")
                            .track_focus(&self.log_focus)
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                log_focus.focus(window, cx);
                            })
                            .cursor_text()
                            .track_scroll(&self.log_scroll)
                            .overflow_y_scroll()
                            .flex_1()
                            .min_h_0()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().muted)
                            .font_family("Menlo")
                            .text_xs()
                            .when(lines.is_empty(), |view| {
                                view.text_color(cx.theme().muted_foreground).child(empty)
                            })
                            .children(lines.into_iter().enumerate().rev().map(|(index, line)| {
                                let color = match line_level(&line) {
                                    Some("ERROR") => Some(cx.theme().danger),
                                    Some("WARN") => Some(cx.theme().warning),
                                    _ => None,
                                };
                                div()
                                    .id(("log-entry", index))
                                    .pb_1()
                                    .when_some(color, |line, color| line.text_color(color))
                                    .child(SelectableText::new(line.clone(), line))
                            })),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(logs.path_label().to_owned()),
                            )
                            .child(
                                Button::new("copy-log")
                                    .icon(IconName::Copy)
                                    .small()
                                    .ghost()
                                    .tooltip("Copy shown lines")
                                    .on_click(self.on_view(|view, cx| view.copy_log_lines(cx))),
                            )
                            .child(action(
                                "open-log",
                                IconName::ExternalLink,
                                "Open log file",
                                SettingsField::OpenLog,
                            ))
                            .child(action(
                                "reveal-log",
                                IconName::FolderOpen,
                                "Show in Finder",
                                SettingsField::RevealLog,
                            ))
                            .child(
                                action(
                                    "clear-log",
                                    IconName::Delete,
                                    "Clear log",
                                    SettingsField::ClearLog,
                                )
                                .danger(),
                            ),
                    ),
            )
    }
}
