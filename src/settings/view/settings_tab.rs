use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length};

use crate::config::AuthMethod;

use super::super::{SettingsField, SettingsForm};

const INPUT_PADDING: u16 = 7;
const LABEL_SIZE: u32 = 12;
const SECTION_SIZE: u32 = 13;

pub(super) fn body<'a>(form: &'a SettingsForm) -> Element<'a, SettingsField> {
    let server_input = input("host.example.com", &form.server, SettingsField::Server);
    let user_input = input("username", &form.username, SettingsField::Username);
    let port_input = input("22", &form.port, SettingsField::Port).width(90);
    let local_input = input("127.0.0.1:1080", &form.local_addr, SettingsField::LocalAddr);

    let fields = column![
        panel(
            "Server",
            column![
                field("Host", server_input.into()),
                row![
                    field("Username", user_input.into()).width(Length::Fill),
                    Space::new().width(10),
                    field("Port", port_input.into()).width(104),
                ]
                .align_y(Alignment::End),
            ]
            .spacing(10)
            .into(),
        ),
        panel("Authentication", auth_fields(form)),
        panel(
            "Local proxy",
            field("SOCKS bind address", local_input.into()).into(),
        ),
    ]
    .spacing(12);

    scrollable(fields).height(Length::Fill).into()
}

pub(super) fn footer<'a>(
    form: &'a SettingsForm,
    config_path: &'a str,
) -> Element<'a, SettingsField> {
    let save = if form.can_save() {
        button("Save").on_press(SettingsField::Save)
    } else {
        button("Save")
    };
    let save_and_start = if form.can_start() {
        button("Save and Start")
            .on_press(SettingsField::SaveAndStart)
            .style(button::primary)
    } else {
        button("Save and Start").style(button::secondary)
    };
    let validation = form
        .save_error()
        .map(|error| format!("Save unavailable: {error}"))
        .or_else(|| {
            form.start_error()
                .map(|error| format!("Save and Start unavailable: {error}"))
        })
        .map(|error| text(error).size(11).wrapping(Wrapping::Word))
        .map(Element::from)
        .unwrap_or_else(|| Space::new().height(0).into());

    column![
        row![
            save,
            save_and_start,
            button("Stop").on_press(SettingsField::Stop),
        ]
        .spacing(8),
        validation,
        text(config_path).size(10).wrapping(Wrapping::Word),
    ]
    .spacing(8)
    .into()
}

fn auth_fields<'a>(form: &'a SettingsForm) -> Element<'a, SettingsField> {
    let is_key = form.auth_method != AuthMethod::Password.as_str();
    let method_row = row![
        button("Public Key")
            .style(if is_key {
                button::primary
            } else {
                button::secondary
            })
            .on_press(SettingsField::AuthMethod(AuthMethod::Key.as_str().into())),
        button("Password")
            .style(if is_key {
                button::secondary
            } else {
                button::primary
            })
            .on_press(SettingsField::AuthMethod(
                AuthMethod::Password.as_str().into()
            )),
    ]
    .spacing(8);

    if form.auth_method == AuthMethod::Password.as_str() {
        let password_input = input(
            "SSH password",
            &form.ssh_password,
            SettingsField::SshPassword,
        )
        .secure(true);
        column![method_row, field("Password", password_input.into())]
            .spacing(10)
            .into()
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
            method_row,
            column![
                label("Private key"),
                row![
                    key_input.width(Length::Fill),
                    Space::new().width(8),
                    button("Choose...").on_press(SettingsField::ChooseKey),
                ]
                .align_y(Alignment::End),
            ]
            .spacing(4),
            field("Key password", key_password_input.into()),
        ]
        .spacing(10)
        .into()
    }
}

fn panel<'a>(title: &'a str, content: Element<'a, SettingsField>) -> Element<'a, SettingsField> {
    container(column![section(title), content].spacing(8))
        .padding(12)
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}

fn field<'a>(
    label_text: &'a str,
    control: Element<'a, SettingsField>,
) -> iced::widget::Column<'a, SettingsField> {
    column![label(label_text), control].spacing(4)
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
