use tray_icon::menu::MenuItem;

use crate::config::AppConfig;

use super::{icons::TrayIconState, stats::StatsSnapshot, tray::TrayMenu};

#[derive(Default)]
pub struct MenuPresenter {
    last_status: String,
    last_stats: String,
    last_config: String,
    last_start_stop: String,
    last_autostart: bool,
    last_auto_connect: bool,
}

impl MenuPresenter {
    pub fn update_tray(
        &mut self,
        tray: &TrayMenu,
        config: &AppConfig,
        stats: &StatsSnapshot,
        running: bool,
    ) {
        let status = match &stats.last_error {
            Some(err) => format!("{err} | Status: {}", stats.status),
            None => format!("Status: {}", stats.status),
        };
        set_menu_text(&tray.status, &mut self.last_status, status);
        set_menu_text(&tray.stats, &mut self.last_stats, stats_summary(stats));
        set_menu_text(&tray.config, &mut self.last_config, config_summary(config));
        set_menu_text(
            &tray.start_stop,
            &mut self.last_start_stop,
            if running { "Stop Proxy" } else { "Start Proxy" },
        );

        if config.autostart != self.last_autostart {
            tray.autostart.set_checked(config.autostart);
            self.last_autostart = config.autostart;
        }
        if config.auto_connect != self.last_auto_connect {
            tray.auto_connect.set_checked(config.auto_connect);
            self.last_auto_connect = config.auto_connect;
        }
    }
}

pub fn settings_status(stats: &StatsSnapshot) -> String {
    format!(
        "{} | SSH: {} ({} total) | up {} | down {}",
        stats.status,
        stats.ssh_current,
        stats.ssh_total,
        format_bytes(stats.bytes_up),
        format_bytes(stats.bytes_down)
    )
}

pub fn icon_state(running: bool, clean: bool) -> TrayIconState {
    if running && clean {
        TrayIconState::Happy
    } else {
        TrayIconState::Unhappy
    }
}

fn stats_summary(stats: &StatsSnapshot) -> String {
    format!(
        "{} SSH ({} total), up {}, down {}",
        stats.ssh_current,
        stats.ssh_total,
        format_bytes(stats.bytes_up),
        format_bytes(stats.bytes_down)
    )
}

fn config_summary(config: &AppConfig) -> String {
    if config.server.is_empty() {
        "No server configured".into()
    } else {
        format!(
            "{}@{}:{} -> {}",
            config.username, config.server, config.port, config.local_addr
        )
    }
}

fn set_menu_text(item: &MenuItem, cached: &mut String, next: impl Into<String>) {
    let next = next.into();
    if next != *cached {
        item.set_text(&next);
        *cached = next;
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}
