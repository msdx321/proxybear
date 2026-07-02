use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Font, Length};

use super::super::{LOG_SCROLL_ID, LogTail, SettingsField};

const SECTION_SIZE: u32 = 13;

pub(super) fn tab<'a>(logs: &'a LogTail) -> Element<'a, SettingsField> {
    let mut lines = column![].spacing(3);
    if logs.lines().is_empty() {
        lines = lines.push(text("No log entries yet").size(13));
    } else {
        for line in logs.lines() {
            lines = lines.push(
                text(line)
                    .size(12)
                    .font(Font::MONOSPACE)
                    .wrapping(Wrapping::Word),
            );
        }
    }

    let error = logs
        .error()
        .map(|error| text(error).size(12))
        .map(Element::from)
        .unwrap_or_else(|| Space::new().height(0).into());

    log_panel(
        "Live logs",
        column![
            row![
                text(logs.status()).size(12),
                Space::new().width(Length::Fill),
                text("proxybear.log").size(11),
            ]
            .align_y(Alignment::Center),
            row![
                button("Open").on_press(SettingsField::OpenLog),
                button("Reveal").on_press(SettingsField::RevealLog),
                button("Clear").on_press(SettingsField::ClearLog),
            ]
            .spacing(8),
            text(logs.path_label()).size(10).wrapping(Wrapping::Word),
            error,
            container(
                scrollable(lines)
                    .id(LOG_SCROLL_ID)
                    .anchor_bottom()
                    .height(Length::Fill)
            )
            .padding(10)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(container::rounded_box),
        ]
        .spacing(8)
        .height(Length::Fill)
        .into(),
    )
}

fn log_panel<'a>(
    title: &'a str,
    content: Element<'a, SettingsField>,
) -> Element<'a, SettingsField> {
    container(
        column![section(title), content]
            .spacing(8)
            .height(Length::Fill),
    )
    .padding(12)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(container::rounded_box)
    .into()
}

fn section(value: &str) -> iced::widget::Text<'_> {
    text(value).size(SECTION_SIZE)
}
