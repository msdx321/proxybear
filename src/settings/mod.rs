mod log_tail;
mod view;

use crate::config::{AppConfig, AuthMethod};

pub use log_tail::LogTail;
pub use view::view;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SettingsTab {
    Settings,
    Logs,
}

#[derive(Debug, Clone)]
pub enum SettingsField {
    Tab(SettingsTab),
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
            auth_method: config.auth_method().as_str().to_string(),
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
        config.set_auth_method(AuthMethod::from_config(&self.auth_method));
        config.key_path = self.key_path.trim().to_string();
        config.key_password.clone_from(&self.key_password);
        config.ssh_password.clone_from(&self.ssh_password);
        config.local_addr = self.local_addr.trim().to_string();
    }
}
