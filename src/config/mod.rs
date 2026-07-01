use std::{
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};

const APP_ID: &str = "com.msdx321.proxybear";

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_path: PathBuf,
    pub launch_agent_path: PathBuf,
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
    let base = BaseDirs::new().context("cannot find home directory")?;
    let home = base.home_dir();
    let config_dir = home.join("Library/Application Support/proxybear");
    Ok(AppPaths {
        config_path: config_dir.join("config.toml"),
        config_dir,
        launch_agent_path: home.join(format!("Library/LaunchAgents/{APP_ID}.plist")),
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

pub fn is_autostart_enabled(paths: &AppPaths) -> bool {
    paths.launch_agent_path.exists()
}

pub fn set_autostart(paths: &AppPaths, enabled: bool) -> Result<()> {
    if enabled {
        let launch_agent_dir = paths
            .launch_agent_path
            .parent()
            .context("LaunchAgent path has no parent directory")?;
        fs::create_dir_all(launch_agent_dir).context("failed to create LaunchAgents directory")?;
        fs::create_dir_all(&paths.config_dir).context("failed to create config directory")?;

        let program_arguments = launch_agent_program_arguments()?;
        let stdout = paths.config_dir.join("proxybear.out.log");
        let stderr = paths.config_dir.join("proxybear.err.log");
        fs::write(
            &paths.launch_agent_path,
            launch_agent_plist(&program_arguments, &stdout, &stderr),
        )
        .context("failed to write LaunchAgent")?;
    } else if paths.launch_agent_path.exists() {
        fs::remove_file(&paths.launch_agent_path).context("failed to remove LaunchAgent")?;
    }
    Ok(())
}

fn launch_agent_program_arguments() -> Result<Vec<String>> {
    let exe = env::current_exe().context("failed to resolve current executable")?;
    if let Some(app) = exe.ancestors().find(|path| {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
    }) {
        return Ok(vec![
            "/usr/bin/open".to_string(),
            "-a".to_string(),
            app.display().to_string(),
        ]);
    }

    Ok(vec![exe.display().to_string()])
}

fn launch_agent_plist(program_arguments: &[String], stdout: &Path, stderr: &Path) -> String {
    let args = program_arguments
        .iter()
        .map(|arg| format!("    <string>{}</string>", xml_escape(arg)))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{APP_ID}</string>
  <key>ProgramArguments</key>
  <array>
{args}
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
        xml_escape(&stdout.display().to_string()),
        xml_escape(&stderr.display().to_string())
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
