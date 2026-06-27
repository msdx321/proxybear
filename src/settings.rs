use iced::widget::{Space, button, column, container, row, rule, text, text_input};
use iced::{Alignment, Element, Length};

use crate::{SettingsField, SettingsForm};

pub fn view<'a>(
    form: &'a SettingsForm,
    stats_text: &'a str,
    config_path: &'a str,
) -> Element<'a, SettingsField> {
    let server_input = text_input("host.example.com", &form.server)
        .on_input(SettingsField::Server)
        .padding(6);
    let user_input = text_input("username", &form.username)
        .on_input(SettingsField::Username)
        .padding(6);
    let port_input = text_input("22", &form.port)
        .on_input(SettingsField::Port)
        .padding(6)
        .width(88);
    let local_input = text_input("127.0.0.1:1080", &form.local_addr)
        .on_input(SettingsField::LocalAddr)
        .padding(6);

    let is_key = form.auth_method != "password";
    let method_row = row![
        button("Public Key")
            .style(if is_key {
                button::primary
            } else {
                button::secondary
            })
            .on_press(SettingsField::AuthMethod("key".into())),
        button("Password")
            .style(if !is_key {
                button::primary
            } else {
                button::secondary
            })
            .on_press(SettingsField::AuthMethod("password".into())),
    ]
    .spacing(0);

    let auth_fields: Element<'a, SettingsField> = if form.auth_method == "password" {
        let pw = text_input("SSH password", &form.ssh_password)
            .on_input(SettingsField::SshPassword)
            .secure(true)
            .padding(6);
        column![text("Password").size(12), pw].spacing(2).into()
    } else {
        let key_input = text_input("/Users/me/.ssh/id_ed25519", &form.key_path)
            .on_input(SettingsField::KeyPath)
            .padding(6);
        let key_pw = text_input("leave empty if none", &form.key_password)
            .on_input(SettingsField::KeyPassword)
            .secure(true)
            .padding(6);
        column![
            text("Private key").size(12),
            row![
                column![key_input].width(Length::Fill),
                Space::new().width(8),
                button("Choose\u{2026}").on_press(SettingsField::ChooseKey),
            ]
            .align_y(Alignment::End),
            Space::new().height(6),
            text("Key password").size(12),
            key_pw,
        ]
        .spacing(2)
        .into()
    };

    container(
        column![
            text("ProxyBear").size(20),
            text(stats_text).size(11),
            rule::horizontal(1),
            // ── Server ──
            text("SERVER").size(11),
            column![text("Host").size(12), server_input].spacing(2),
            row![
                column![text("Username").size(12), user_input].width(Length::Fill),
                Space::new().width(10),
                column![text("Port").size(12), port_input],
            ],
            Space::new().height(4),
            rule::horizontal(1),
            // ── Authentication ──
            text("AUTHENTICATION").size(11),
            method_row,
            auth_fields,
            Space::new().height(4),
            rule::horizontal(1),
            // ── Local ──
            text("LOCAL").size(11),
            column![text("SOCKS bind address").size(12), local_input].spacing(2),
            Space::new().height(4),
            rule::horizontal(1),
            // ── Actions ──
            row![
                button("Save").on_press(SettingsField::Save),
                button("Save and Start")
                    .on_press(SettingsField::SaveAndStart)
                    .style(button::primary),
                button("Stop").on_press(SettingsField::Stop),
            ]
            .spacing(8),
            // Config path
            text(config_path).size(10),
        ]
        .spacing(10),
    )
    .padding(20)
    .width(440)
    .into()
}
