use std::{env, fs, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result, bail};
use auto_launch::{AutoLaunch, AutoLaunchBuilder, MacOSLaunchMode, WindowsEnableMode};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const APP_ID: &str = "com.msdx321.proxybear";

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_path: PathBuf,
}

impl AppPaths {
    pub fn log_path(&self) -> PathBuf {
        self.config_dir.join("proxybear.log")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthMethod {
    Key,
    Password,
}

impl AuthMethod {
    pub fn from_config(value: &str) -> Self {
        match value {
            "password" => Self::Password,
            _ => Self::Key,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::Password => "password",
        }
    }

    fn requires_key(self) -> bool {
        matches!(self, Self::Key)
    }
}

#[derive(Clone, Debug)]
pub struct ListenConfig {
    pub local_addr: SocketAddr,
}

#[derive(Clone, Debug)]
pub struct SshConnectConfig {
    pub server: String,
    pub username: String,
    pub port: u16,
    pub auth_method: AuthMethod,
    pub key_path: String,
    pub key_password: String,
    pub ssh_password: String,
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub listen: ListenConfig,
    pub ssh: SshConnectConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    pub server: String,
    pub username: String,
    pub port: u16,
    #[serde(default)]
    pub auth_method: String,
    pub key_path: String,
    #[serde(default)]
    pub key_password: String,
    #[serde(default)]
    pub ssh_password: String,
    pub local_addr: String,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub auto_connect: bool,
    pub host_fingerprint: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: String::new(),
            username: env::var("USER").unwrap_or_default(),
            port: 22,
            auth_method: AuthMethod::Key.as_str().into(),
            key_path: String::new(),
            key_password: String::new(),
            ssh_password: String::new(),
            local_addr: "127.0.0.1:1080".to_string(),
            autostart: false,
            auto_connect: false,
            host_fingerprint: None,
        }
    }
}

impl AppConfig {
    pub fn auth_method(&self) -> AuthMethod {
        AuthMethod::from_config(&self.auth_method)
    }

    pub fn set_auth_method(&mut self, method: AuthMethod) {
        self.auth_method = method.as_str().to_string();
    }

    pub fn validate_ready(&self) -> Result<()> {
        self.runtime_config().map(|_| ())
    }

    pub fn runtime_config(&self) -> Result<RuntimeConfig> {
        let server = self.server.trim();
        if server.is_empty() {
            bail!("server is empty");
        }
        let username = self.username.trim();
        if username.is_empty() {
            bail!("username is empty");
        }
        let auth_method = self.auth_method();
        let key_path = self.key_path.trim();
        if auth_method.requires_key() && key_path.is_empty() {
            bail!("key path is empty");
        }

        let local_addr = self
            .local_addr
            .trim()
            .parse::<SocketAddr>()
            .with_context(|| format!("invalid local address {}", self.local_addr))?;
        Ok(RuntimeConfig {
            listen: ListenConfig { local_addr },
            ssh: SshConnectConfig {
                server: server.to_string(),
                username: username.to_string(),
                port: self.port,
                auth_method,
                key_path: key_path.to_string(),
                key_password: self.key_password.clone(),
                ssh_password: self.ssh_password.clone(),
            },
        })
    }
}

pub fn app_paths() -> Result<AppPaths> {
    let project_dirs =
        ProjectDirs::from("", "", "proxybear").context("cannot find app directories")?;
    let config_dir = project_dirs.config_dir().to_path_buf();
    Ok(AppPaths {
        config_path: config_dir.join("config.toml"),
        config_dir,
    })
}

pub fn load_config(paths: &AppPaths) -> Result<AppConfig> {
    let mut config = match fs::read_to_string(&paths.config_path) {
        Ok(text) => toml::from_str(&text).context("invalid config TOML")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppConfig::default(),
        Err(error) => return Err(error).context("failed to read config"),
    };
    config.autostart = is_autostart_enabled(paths);
    Ok(config)
}

pub fn save_config(paths: &AppPaths, config: &AppConfig) -> Result<()> {
    fs::create_dir_all(&paths.config_dir).context("failed to create config directory")?;
    let text = toml::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(&paths.config_path, text).context("failed to write config")
}

pub fn is_autostart_enabled(_paths: &AppPaths) -> bool {
    autostart().is_ok_and(|autostart| autostart.is_enabled().unwrap_or(false))
}

pub fn set_autostart(_paths: &AppPaths, enabled: bool) -> Result<()> {
    let autostart = autostart()?;
    if enabled {
        autostart.enable().context("failed to enable autostart")?;
    } else {
        autostart.disable().context("failed to disable autostart")?;
    }
    Ok(())
}

fn autostart() -> Result<AutoLaunch> {
    let app_path = env::current_exe()
        .context("failed to resolve current executable")?
        .display()
        .to_string();
    let mut builder = AutoLaunchBuilder::new();
    builder
        .set_app_name(APP_ID)
        .set_app_path(&app_path)
        .set_macos_launch_mode(MacOSLaunchMode::LaunchAgent)
        .set_bundle_identifiers(&[APP_ID])
        .set_windows_enable_mode(WindowsEnableMode::CurrentUser);
    builder.build().context("failed to configure autostart")
}
