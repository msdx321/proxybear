mod log_tail;
mod theme;
mod view;

use std::net::SocketAddr;

use anyhow::{Context, Result};

use crate::config::{AppConfig, AuthMethod, LogLevel};

pub use log_tail::LogTail;
pub use theme::init as init_theme;
pub use view::SettingsView;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SettingsTab {
    Settings,
    Logs,
}

#[derive(Debug, Clone)]
pub enum SettingsField {
    Tab(SettingsTab),
    LogLevel(LogLevel),
    Server(String),
    Username(String),
    Port(String),
    AuthMethod(AuthMethod),
    KeyPath(String),
    KeyPassword(String),
    SshPassword(String),
    LocalAddr(String),
    Save,
    SaveAndStart,
    Stop,
    ChooseKey,
    OpenLog,
    RevealLog,
    ClearLog,
}

#[derive(Debug, Clone)]
pub struct SettingsForm {
    pub server: String,
    pub username: String,
    pub port: String,
    pub auth_method: AuthMethod,
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
            auth_method: config.auth_method(),
            key_path: config.key_path.clone(),
            key_password: config.key_password.clone(),
            ssh_password: config.ssh_password.clone(),
            local_addr: config.local_addr.clone(),
        }
    }

    pub fn apply_to_config(&self, config: &mut AppConfig) -> Result<()> {
        config.server = self.server.trim().to_string();
        config.username = self.username.trim().to_string();
        config.port = self.parse_port()?;
        config.set_auth_method(self.auth_method);
        config.key_path = self.key_path.trim().to_string();
        config.key_password.clone_from(&self.key_password);
        config.ssh_password.clone_from(&self.ssh_password);
        config.local_addr = self.parse_local_addr()?.to_string();
        Ok(())
    }

    pub fn save_error(&self) -> Option<String> {
        self.parse_port()
            .and_then(|_| self.parse_local_addr())
            .err()
            .map(|error| error.to_string())
    }

    pub fn connection_error(&self) -> Option<&'static str> {
        if self.server.trim().is_empty() {
            Some("Enter the SSH server hostname.")
        } else if self.username.trim().is_empty() {
            Some("Enter your SSH username.")
        } else if self.auth_method == AuthMethod::Key && self.key_path.trim().is_empty() {
            Some("Choose an SSH private key file.")
        } else {
            None
        }
    }

    fn parse_port(&self) -> Result<u16> {
        let port = self.port.trim();
        port.parse()
            .with_context(|| format!("invalid SSH port {port}"))
    }

    fn parse_local_addr(&self) -> Result<SocketAddr> {
        let local_addr = self.local_addr.trim();
        local_addr
            .parse()
            .with_context(|| format!("invalid SOCKS bind address {local_addr}"))
    }
}
