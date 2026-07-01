mod logs;
mod settings_tab;

use iced::widget::{Space, button, column, container, row, rule, text};
use iced::{Alignment, Element, Length};

use super::{LogTail, SettingsField, SettingsForm, SettingsTab};

const FORM_WIDTH: f32 = 520.0;

pub fn view<'a>(
    form: &'a SettingsForm,
    active_tab: SettingsTab,
    logs: &'a LogTail,
    stats_text: &'a str,
    config_path: &'a str,
) -> Element<'a, SettingsField> {
    let body = match active_tab {
        SettingsTab::Settings => settings_tab::body(form),
        SettingsTab::Logs => logs::tab(logs),
    };
    let content = match active_tab {
        SettingsTab::Settings => {
            column![body, rule::horizontal(1), settings_tab::footer(config_path)]
                .spacing(12)
                .height(Length::Fill)
                .into()
        }
        SettingsTab::Logs => body,
    };

    container(
        column![header(stats_text), tab_bar(active_tab), content]
            .spacing(14)
            .height(Length::Fill),
    )
    .padding(20)
    .width(FORM_WIDTH)
    .height(Length::Fill)
    .into()
}

fn header<'a>(stats_text: &'a str) -> Element<'a, SettingsField> {
    row![
        column![text("ProxyBear").size(22), text(stats_text).size(11)].spacing(2),
        Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center)
    .into()
}

fn tab_bar(active_tab: SettingsTab) -> Element<'static, SettingsField> {
    row![
        tab_button("Settings", SettingsTab::Settings, active_tab),
        tab_button("Logs", SettingsTab::Logs, active_tab),
    ]
    .spacing(8)
    .into()
}

fn tab_button(
    label: &'static str,
    tab: SettingsTab,
    active_tab: SettingsTab,
) -> iced::widget::Button<'static, SettingsField> {
    button(text(label).size(13))
        .style(if tab == active_tab {
            button::primary
        } else {
            button::secondary
        })
        .padding([7, 14])
        .on_press(SettingsField::Tab(tab))
}
