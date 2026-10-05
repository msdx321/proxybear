mod keychain;

use std::{env, fs, io::Write, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result, bail};
use auto_launch::{AutoLaunch, AutoLaunchBuilder, MacOSLaunchMode, WindowsEnableMode};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const APP_ID: &str = "com.msdx321.proxybear";
/// Number of SSH sessions kept open when the config does not set one.
pub const DEFAULT_POOL_SIZE: usize = 3;
pub const MAX_POOL_SIZE: usize = 16;

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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    // Older versions could save an empty method, which meant key auth.
    #[default]
    #[serde(alias = "")]
    Key,
    Password,
}

impl AuthMethod {
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
    /// The trusted host key, if one was saved.
    pub host_fingerprint: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub listen: ListenConfig,
    pub ssh: SshConnectConfig,
    pub pool_size: usize,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    #[default]
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn filter(self) -> tracing_subscriber::filter::LevelFilter {
        use tracing_subscriber::filter::LevelFilter;

        match self {
            Self::Error => LevelFilter::ERROR,
            Self::Warn => LevelFilter::WARN,
            Self::Info => LevelFilter::INFO,
            Self::Debug => LevelFilter::DEBUG,
            Self::Trace => LevelFilter::TRACE,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    pub server: String,
    pub username: String,
    pub port: u16,
    #[serde(default = "default_pool_size")]
    pub pool_size: usize,
    #[serde(default)]
    pub auth_method: AuthMethod,
    pub key_path: String,
    /// Kept in the Keychain. Older versions saved it in the file, so it is
    /// still read from there once to migrate it.
    #[serde(default, skip_serializing)]
    pub key_password: String,
    /// Kept in the Keychain, like `key_password`.
    #[serde(default, skip_serializing)]
    pub ssh_password: String,
    pub local_addr: SocketAddr,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub auto_connect: bool,
    #[serde(default)]
    pub log_level: LogLevel,
    pub host_fingerprint: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: String::new(),
            username: env::var("USER").unwrap_or_default(),
            port: 22,
            pool_size: DEFAULT_POOL_SIZE,
            auth_method: AuthMethod::default(),
            key_path: String::new(),
            key_password: String::new(),
            ssh_password: String::new(),
            local_addr: SocketAddr::from(([127, 0, 0, 1], 1080)),
            autostart: false,
            auto_connect: false,
            log_level: LogLevel::default(),
            host_fingerprint: None,
        }
    }
}

impl AppConfig {
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
        let auth_method = self.auth_method;
        let key_path = self.key_path.trim();
        if auth_method.requires_key() && key_path.is_empty() {
            bail!("key path is empty");
        }

        if !(1..=MAX_POOL_SIZE).contains(&self.pool_size) {
            bail!("SSH session count must be between 1 and {MAX_POOL_SIZE}");
        }

        Ok(RuntimeConfig {
            listen: ListenConfig {
                local_addr: self.local_addr,
            },
            ssh: SshConnectConfig {
                server: server.to_string(),
                username: username.to_string(),
                port: self.port,
                auth_method,
                key_path: key_path.to_string(),
                key_password: self.key_password.clone(),
                ssh_password: self.ssh_password.clone(),
                host_fingerprint: self.host_fingerprint.clone(),
            },
            pool_size: self.pool_size,
        })
    }
}

fn default_pool_size() -> usize {
    DEFAULT_POOL_SIZE
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

/// Write the config file, readable only by the user. Secrets are left out;
/// see [`save_secrets`].
pub fn save_config(paths: &AppPaths, config: &AppConfig) -> Result<()> {
    fs::create_dir_all(&paths.config_dir).context("failed to create config directory")?;
    let text = toml::to_string_pretty(config).context("failed to serialize config")?;
    // A temporary file is created with mode 0600 and renamed into place, so
    // a crash never leaves a half-written config behind.
    let mut file = tempfile::NamedTempFile::new_in(&paths.config_dir)
        .context("failed to create temporary config file")?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.as_file().sync_all())
        .context("failed to write config")?;
    file.persist(&paths.config_path)
        .context("failed to replace config")?;
    Ok(())
}

/// Fill in the secrets from the Keychain, first moving any that an older
/// version left in the config file.
pub fn load_secrets(paths: &AppPaths, config: &mut AppConfig) -> Result<()> {
    if !config.ssh_password.is_empty() || !config.key_password.is_empty() {
        keychain::set(keychain::SSH_PASSWORD, &config.ssh_password)?;
        keychain::set(keychain::KEY_PASSPHRASE, &config.key_password)?;
        return save_config(paths, config).context("failed to remove secrets from config");
    }
    config.ssh_password = keychain::get(keychain::SSH_PASSWORD)?;
    config.key_password = keychain::get(keychain::KEY_PASSPHRASE)?;
    Ok(())
}

/// Store the secrets that differ between `old` and `new` in the Keychain.
pub fn save_secrets(old: &AppConfig, new: &AppConfig) -> Result<()> {
    if new.ssh_password != old.ssh_password {
        keychain::set(keychain::SSH_PASSWORD, &new.ssh_password)?;
    }
    if new.key_password != old.key_password {
        keychain::set(keychain::KEY_PASSPHRASE, &new.key_password)?;
    }
    Ok(())
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
