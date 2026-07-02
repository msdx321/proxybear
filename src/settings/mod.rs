mod log_tail;
mod view;

use std::net::SocketAddr;

use anyhow::{Context, Result, bail};

use crate::config::{AppConfig, AuthMethod};

pub use log_tail::LogTail;
pub use view::view;

pub const LOG_SCROLL_ID: &str = "logs-scroll";

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
    OpenLog,
    RevealLog,
    ClearLog,
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

    pub fn apply_to_config(&self, config: &mut AppConfig) -> Result<()> {
        config.server = self.server.trim().to_string();
        config.username = self.username.trim().to_string();
        config.port = self.parse_port()?;
        config.set_auth_method(AuthMethod::from_config(&self.auth_method));
        config.key_path = self.key_path.trim().to_string();
        config.key_password.clone_from(&self.key_password);
        config.ssh_password.clone_from(&self.ssh_password);
        config.local_addr = self.parse_local_addr()?.to_string();
        Ok(())
    }

    pub fn save_error(&self) -> Option<String> {
        self.validate_save().err().map(|error| error.to_string())
    }

    pub fn start_error(&self) -> Option<String> {
        self.validate_start().err().map(|error| error.to_string())
    }

    pub fn can_save(&self) -> bool {
        self.validate_save().is_ok()
    }

    pub fn can_start(&self) -> bool {
        self.validate_start().is_ok()
    }

    fn validate_save(&self) -> Result<()> {
        self.parse_port()?;
        self.parse_local_addr()?;
        Ok(())
    }

    fn validate_start(&self) -> Result<()> {
        self.validate_save()?;
        if self.server.trim().is_empty() {
            bail!("server is empty");
        }
        if self.username.trim().is_empty() {
            bail!("username is empty");
        }
        if self.auth_method != AuthMethod::Password.as_str() && self.key_path.trim().is_empty() {
            bail!("key path is empty");
        }
        Ok(())
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
