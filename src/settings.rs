use iced::widget::{Space, button, column, container, row, rule, text, text_input};
use iced::{Alignment, Element, Length};

use crate::config::AppConfig;

const INPUT_PADDING: u16 = 6;
const LABEL_SIZE: u32 = 12;
const SECTION_SIZE: u32 = 11;
const FORM_WIDTH: f32 = 440.0;

#[derive(Debug, Clone)]
pub enum SettingsField {
    Server(String),
    Username(String),
    Port(String),
    AuthMethod(String),
    KeyPath(String),
    KeyPassword(String),
    SshPassword(String),
    LocalAddr(String),
    Save,
    SaveAndStart,
    Stop,
    ChooseKey,
}

#[derive(Debug, Clone)]
pub struct SettingsForm {
    pub server: String,
    pub username: String,
    pub port: String,
    pub auth_method: String,
    pub key_path: String,
    pub key_password: String,
    pub ssh_password: String,
    pub local_addr: String,
}

impl SettingsForm {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            server: config.server.clone(),
            username: config.username.clone(),
            port: config.port.to_string(),
            auth_method: config.auth_method.clone(),
            key_path: config.key_path.clone(),
            key_password: config.key_password.clone(),
            ssh_password: config.ssh_password.clone(),
            local_addr: config.local_addr.clone(),
        }
    }

    pub fn apply_to_config(&self, config: &mut AppConfig) {
        config.server = self.server.trim().to_string();
        config.username = self.username.trim().to_string();
        config.port = self.port.parse().unwrap_or(22);
        config.auth_method = if self.auth_method == "password" {
            "password".into()
        } else {
            "key".into()
        };
        config.key_path = self.key_path.trim().to_string();
        config.key_password.clone_from(&self.key_password);
        config.ssh_password.clone_from(&self.ssh_password);
        config.local_addr = self.local_addr.trim().to_string();
    }
}

pub fn view<'a>(
    form: &'a SettingsForm,
    stats_text: &'a str,
    config_path: &'a str,
) -> Element<'a, SettingsField> {
    let server_input = input("host.example.com", &form.server, SettingsField::Server);
    let user_input = input("username", &form.username, SettingsField::Username);
    let port_input = input("22", &form.port, SettingsField::Port).width(88);
    let local_input = input("127.0.0.1:1080", &form.local_addr, SettingsField::LocalAddr);

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
            .style(if is_key {
                button::secondary
            } else {
                button::primary
            })
            .on_press(SettingsField::AuthMethod("password".into())),
    ]
    .spacing(0);

    let auth_fields: Element<'a, SettingsField> = if form.auth_method == "password" {
        let password_input = input(
            "SSH password",
            &form.ssh_password,
            SettingsField::SshPassword,
        )
        .secure(true);
        column![label("Password"), password_input].spacing(2).into()
    } else {
        let key_input = input(
            "/Users/me/.ssh/id_ed25519",
            &form.key_path,
            SettingsField::KeyPath,
        );
        let key_password_input = input(
            "leave empty if none",
            &form.key_password,
            SettingsField::KeyPassword,
        )
        .secure(true);
        column![
            label("Private key"),
            row![
                column![key_input].width(Length::Fill),
                Space::new().width(8),
                button("Choose\u{2026}").on_press(SettingsField::ChooseKey),
            ]
            .align_y(Alignment::End),
            Space::new().height(6),
            label("Key password"),
            key_password_input,
        ]
        .spacing(2)
        .into()
    };

    container(
        column![
            text("ProxyBear").size(20),
            text(stats_text).size(11),
            rule::horizontal(1),
            section("SERVER"),
            column![label("Host"), server_input].spacing(2),
            row![
                column![label("Username"), user_input].width(Length::Fill),
                Space::new().width(10),
                column![label("Port"), port_input],
            ],
            Space::new().height(4),
            rule::horizontal(1),
            section("AUTHENTICATION"),
            method_row,
            auth_fields,
            Space::new().height(4),
            rule::horizontal(1),
            section("LOCAL"),
            column![label("SOCKS bind address"), local_input].spacing(2),
            Space::new().height(4),
            rule::horizontal(1),
            row![
                button("Save").on_press(SettingsField::Save),
                button("Save and Start")
                    .on_press(SettingsField::SaveAndStart)
                    .style(button::primary),
                button("Stop").on_press(SettingsField::Stop),
            ]
            .spacing(8),
            text(config_path).size(10),
        ]
        .spacing(10),
    )
    .padding(20)
    .width(FORM_WIDTH)
    .into()
}

fn input<'a>(
    placeholder: &'a str,
    value: &'a str,
    on_input: impl Fn(String) -> SettingsField + 'a,
) -> iced::widget::TextInput<'a, SettingsField> {
    text_input(placeholder, value)
        .on_input(on_input)
        .padding(INPUT_PADDING)
}

fn label(value: &str) -> iced::widget::Text<'_> {
    text(value).size(LABEL_SIZE)
}

fn section(value: &str) -> iced::widget::Text<'_> {
    text(value).size(SECTION_SIZE)
}
