mod log_tail;
mod theme;
mod view;

use std::net::SocketAddr;

use anyhow::{Context, Result};

use crate::config::{AppConfig, AuthMethod, LogLevel, MAX_POOL_SIZE};

pub use log_tail::LogTail;
pub use theme::init as init_theme;
pub use view::SettingsView;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SettingsTab {
    General,
    Connection,
    Logs,
}

#[derive(Debug, Clone)]
pub enum SettingsField {
    Tab(SettingsTab),
    LogLevel(LogLevel),
    Server(String),
    Username(String),
    Port(String),
    PoolSize(String),
    AuthMethod(AuthMethod),
    KeyPath(String),
    KeyPassword(String),
    SshPassword(String),
    LocalAddr(String),
    Autostart(bool),
    AutoConnect(bool),
    Save,
    SaveAndStart,
    Revert,
    Start,
    Stop,
    ForgetHostKey,
    ChooseKey,
    OpenLog,
    RevealLog,
    RevealConfig,
    ClearLog,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SettingsForm {
    pub server: String,
    pub username: String,
    pub port: String,
    pub pool_size: String,
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
            pool_size: config.pool_size.to_string(),
            auth_method: config.auth_method,
            key_path: config.key_path.clone(),
            key_password: config.key_password.clone(),
            ssh_password: config.ssh_password.clone(),
            local_addr: config.local_addr.to_string(),
        }
    }

    pub fn apply_to_config(&self, config: &mut AppConfig) -> Result<()> {
        let server = self.server.trim();
        let port = self.parse_port()?;
        if server != config.server || port != config.port {
            // The saved host key belongs to the previous server.
            config.host_fingerprint = None;
        }
        config.server = server.to_string();
        config.username = self.username.trim().to_string();
        config.port = port;
        config.pool_size = self.parse_pool_size()?;
        config.auth_method = self.auth_method;
        config.key_path = self.key_path.trim().to_string();
        config.key_password.clone_from(&self.key_password);
        config.ssh_password.clone_from(&self.ssh_password);
        config.local_addr = self.parse_local_addr()?;
        Ok(())
    }

    pub fn save_error(&self) -> Option<String> {
        self.parse_port()
            .and_then(|_| self.parse_pool_size())
            .and_then(|_| self.parse_local_addr())
            .err()
            .map(|error| error.to_string())
    }

    /// Whether the bind address lets other devices use the proxy.
    pub fn exposes_proxy(&self) -> bool {
        self.parse_local_addr()
            .is_ok_and(|addr| !addr.ip().is_loopback())
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

    fn parse_pool_size(&self) -> Result<usize> {
        let pool_size = self.pool_size.trim();
        pool_size
            .parse()
            .ok()
            .filter(|size| (1..=MAX_POOL_SIZE).contains(size))
            .with_context(|| {
                format!("SSH session count must be between 1 and {MAX_POOL_SIZE}, got {pool_size}")
            })
    }

    fn parse_local_addr(&self) -> Result<SocketAddr> {
        let local_addr = self.local_addr.trim();
        local_addr
            .parse()
            .with_context(|| format!("invalid SOCKS bind address {local_addr}"))
    }
}
