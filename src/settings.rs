
use iced::widget::{button, column, container, row, text, text_input, toggler, Space};
use iced::{Alignment, Element, Length};

use crate::{SettingsField, SettingsForm};

pub fn view<'a>(form: &'a SettingsForm, stats_text: &'a str, config_path: &'a str) -> Element<'a, SettingsField> {
    let server_input = text_input("host.example.com", &form.server)
        .on_input(SettingsField::Server).padding(6);
    let user_input = text_input("username", &form.username)
        .on_input(SettingsField::Username).padding(6);
    let port_input = text_input("22", &form.port)
        .on_input(SettingsField::Port).padding(6).width(88);
    let key_input = text_input("/Users/me/.ssh/id_ed25519", &form.key_path)
        .on_input(SettingsField::KeyPath).padding(6);
    let local_input = text_input("127.0.0.1:1080", &form.local_addr)
        .on_input(SettingsField::LocalAddr).padding(6);

    container(
        column![
            text("ProxyBear").size(20),
            text(stats_text).size(12),
            // Server
            column![text("Server host").size(12), server_input].spacing(2),
            // User + Port
            row![
                column![text("User").size(12), user_input].width(Length::Fill),
                Space::new().width(10),
                column![text("SSH port").size(12), port_input],
            ],
            // Key
            column![
                text("Private key").size(12),
                row![
                    column![key_input].width(Length::Fill),
                    Space::new().width(8),
                    button("Choose\u{2026}").on_press(SettingsField::ChooseKey),
                ].align_y(Alignment::End),
            ].spacing(2),
            // Local bind
            column![text("SOCKS bind").size(12), local_input].spacing(2),
            // Autostart
            toggler(form.autostart).on_toggle(SettingsField::Autostart).label("Launch at login"),
            // Buttons
            row![
                button("Save").on_press(SettingsField::Save),
                button("Save and Start").on_press(SettingsField::SaveAndStart).style(button::primary),
                button("Stop").on_press(SettingsField::Stop),
            ].spacing(8),
            // Config path
            text(config_path).size(10),
        ].spacing(12),
    )
    .padding(20)
    .width(440)
    .into()
}
