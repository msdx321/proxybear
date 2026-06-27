use std::{
    env, fs,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    pub server: String,
    pub username: String,
    pub port: u16,
    pub key_path: String,
    pub local_addr: String,
    pub autostart: bool,
    pub auto_connect: bool,
    pub host_fingerprint: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: String::new(),
            username: env::var("USER").unwrap_or_default(),
            port: 22,
            key_path: String::new(),
            local_addr: "127.0.0.1:1080".to_string(),
            autostart: false,
            auto_connect: false,
            host_fingerprint: None,
        }
    }
}

impl AppConfig {
    pub fn validate_ready(&self) -> Result<()> {
        if self.server.trim().is_empty() {
            bail!("server is empty");
        }
        if self.username.trim().is_empty() {
            bail!("username is empty");
        }
        if self.key_path.trim().is_empty() {
            bail!("key path is empty");
        }
        self.local_addr
            .parse::<std::net::SocketAddr>()
            .with_context(|| format!("invalid local address {}", self.local_addr))?;
        Ok(())
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
        fs::create_dir_all(paths.launch_agent_path.parent().unwrap())
            .context("failed to create LaunchAgents directory")?;
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
