mod app;
mod config;
mod proxy;
mod settings;

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use futures::StreamExt;
use gpui::{AppContext, Bounds, WindowBounds, WindowHandle, WindowOptions, px, size};
use gpui_component::Root;
use native_dialog::DialogBuilder;

use app::{
    logging, platform, presentation,
    presentation::MenuPresenter,
    proxy_control::ProxyController,
    stats::{self, ProxyStats, StatsSnapshot},
    tray::{self, MenuAction, TrayMenu},
};
use config::{AppConfig, AppPaths, app_paths, load_config, save_config};
use settings::{LogTail, SettingsField, SettingsForm, SettingsTab, SettingsView};

struct ProxyBear {
    paths: AppPaths,
    config: Arc<Mutex<AppConfig>>,
    stats: Arc<ProxyStats>,
    proxy: ProxyController,
    tray: TrayMenu,
    menu: MenuPresenter,
    form: SettingsForm,
    active_tab: SettingsTab,
    log_tail: LogTail,
    stats_text: String,
    config_path: String,
    feedback: Option<String>,
    settings_window: Option<WindowHandle<Root>>,
    menu_open: bool,
}

impl ProxyBear {
    fn new() -> Result<Self> {
        platform::activate_as_accessory();
        let paths = app_paths().context("app paths")?;
        logging::init(&paths.config_dir).context("open log file")?;
        tracing::info!(event = "app_started", "ProxyBear starting");
        let config = load_config(&paths).context("load config")?;
        let stats = Arc::new(ProxyStats::default());
        stats.set_status("Stopped");
        let proxy = ProxyController::new().context("create proxy controller")?;
        let tray = TrayMenu::new(&paths, config.auto_connect).context("tray menu")?;
        let config_path = paths.config_path.display().to_string();
        let form = SettingsForm::from_config(&config);
        let log_tail = LogTail::new(paths.log_path());
        Ok(Self {
            paths,
            config: Arc::new(Mutex::new(config)),
            stats,
            proxy,
            tray,
            menu: MenuPresenter::default(),
            form,
            active_tab: SettingsTab::Settings,
            log_tail,
            stats_text: String::new(),
            config_path,
            feedback: None,
            settings_window: None,
            menu_open: false,
        })
    }

    fn listen(&mut self, cx: &mut gpui::Context<Self>) {
        let mut menus = tray::subscribe();
        cx.spawn(async move |this, cx| {
            while let Some(action) = menus.next().await {
                if this
                    .update(cx, |this, cx| this.handle_menu(action, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let mut stats = stats::subscribe();
        cx.spawn(async move |this, cx| {
            while stats.next().await.is_some() {
                if this
                    .update(cx, |this, cx| {
                        this.refresh_stats();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let mut logs = logging::subscribe();
        cx.spawn(async move |this, cx| {
            while logs.changed().await.is_ok() {
                gpui::Timer::after(Duration::from_secs(1)).await;
                logs.borrow_and_update();
                if this
                    .update(cx, |this, cx| {
                        if this.settings_window.is_some() && this.active_tab == SettingsTab::Logs {
                            this.log_tail.refresh();
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                gpui::Timer::after(Duration::from_secs(5)).await;
                if this
                    .update(cx, |this, cx| {
                        if this.proxy.is_running()
                            && (this.settings_window.is_some() || this.menu_open)
                        {
                            this.refresh_stats();
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        self.refresh_stats();
        if self.config_snapshot().auto_connect {
            self.start_proxy(cx);
        }
    }

    fn handle_menu(&mut self, action: MenuAction, cx: &mut gpui::Context<Self>) {
        match action {
            MenuAction::MenuOpened => {
                self.menu_open = true;
                self.refresh_stats();
            }
            MenuAction::MenuClosed => self.menu_open = false,
            MenuAction::StartStop => {
                if self.proxy.is_running() {
                    self.stop_proxy();
                } else {
                    self.start_proxy(cx);
                }
            }
            MenuAction::Settings => self.open_settings(cx),
            MenuAction::ToggleAutostart => {
                let mut config = self.config_snapshot();
                config.autostart = !config.autostart;
                if let Err(error) = config::set_autostart(&self.paths, config.autostart)
                    .and_then(|()| self.save_config_state(config))
                {
                    self.stats.set_error(error.to_string());
                }
            }
            MenuAction::ToggleAutoConnect => {
                let mut config = self.config_snapshot();
                config.auto_connect = !config.auto_connect;
                if let Err(error) = self.save_config_state(config) {
                    self.stats.set_error(error.to_string());
                }
            }
            MenuAction::Quit => {
                self.stop_proxy();
                cx.quit();
            }
        }
        self.refresh_stats();
        cx.notify();
    }

    fn handle_field(&mut self, field: SettingsField, cx: &mut gpui::Context<Self>) {
        self.feedback = None;
        match field {
            SettingsField::Tab(tab) => {
                self.active_tab = tab;
                if tab == SettingsTab::Logs {
                    self.log_tail.refresh();
                }
            }
            SettingsField::Server(v) => self.form.server = v,
            SettingsField::Username(v) => self.form.username = v,
            SettingsField::Port(v) => self.form.port = v,
            SettingsField::AuthMethod(v) => self.form.auth_method = v,
            SettingsField::KeyPath(v) => self.form.key_path = v,
            SettingsField::KeyPassword(v) => self.form.key_password = v,
            SettingsField::SshPassword(v) => self.form.ssh_password = v,
            SettingsField::LocalAddr(v) => self.form.local_addr = v,
            SettingsField::Save | SettingsField::SaveAndStart => {
                let start = matches!(field, SettingsField::SaveAndStart);
                match self.save_settings() {
                    Ok(()) => {
                        self.feedback = Some("Settings saved".into());
                        if start {
                            self.start_proxy(cx);
                        }
                    }
                    Err(error) => self.stats.set_error(error.to_string()),
                }
            }
            SettingsField::Stop => self.stop_proxy(),
            SettingsField::ChooseKey => self.choose_key(),
            SettingsField::OpenLog => self.open_log(),
            SettingsField::RevealLog => self.reveal_log(),
            SettingsField::ClearLog => {
                if let Err(error) = self.log_tail.clear() {
                    self.stats
                        .set_error(format!("failed to clear log: {error}"));
                }
            }
        }
        self.refresh_stats();
        cx.notify();
    }

    fn open_settings(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some(handle) = self.settings_window {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                cx.activate(true);
                return;
            }
            self.settings_window = None;
        }
        self.refresh_stats();
        self.log_tail.refresh();
        let app = cx.entity();
        // Defer construction so input initialization can read the app entity.
        cx.defer(move |cx| {
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(760.), px(780.)),
                        cx,
                    ))),
                    window_min_size: Some(size(px(640.), px(600.))),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("ProxyBear Settings".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    let weak = app.downgrade();
                    window.on_window_should_close(cx, move |_, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.settings_window = None;
                            cx.notify();
                        });
                        true
                    });
                    let view = cx.new(|cx| SettingsView::new(app.clone(), window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            );
            app.update(cx, |this, cx| {
                match result {
                    Ok(handle) => {
                        this.settings_window = Some(handle);
                        cx.activate(true);
                    }
                    Err(error) => this
                        .stats
                        .set_error(format!("failed to open settings: {error}")),
                }
                cx.notify();
            });
        });
    }
}

impl ProxyBear {
    fn refresh_stats(&mut self) {
        let stats = self.stats.snapshot();
        let running = self.proxy.is_running();
        self.stats_text = presentation::settings_status(&stats);
        self.update_icon_for(&stats, running);

        let config = self.config_snapshot();
        self.menu.update_tray(&self.tray, &config, &stats, running);
    }

    fn update_icon_for(&self, stats: &StatsSnapshot, running: bool) {
        let clean = stats.last_error.is_none() && stats.ssh_connected;
        let _ = self
            .tray
            .set_icon_state(presentation::icon_state(running, clean));
    }
}

impl ProxyBear {
    fn start_proxy(&mut self, cx: &mut gpui::Context<Self>) {
        match self.proxy.start(
            Arc::clone(&self.config),
            self.paths.clone(),
            Arc::clone(&self.stats),
        ) {
            Ok(Some(task)) => {
                cx.spawn(async move |this, cx| {
                    let result = task
                        .await
                        .context("proxy task failed")
                        .and_then(|result| result);
                    let _ = this.update(cx, |this, cx| {
                        this.proxy.finish();
                        this.stats.set_status("Stopped");
                        if let Err(error) = result {
                            this.stats.set_error(error.to_string());
                        }
                        this.refresh_stats();
                        cx.notify();
                    });
                })
                .detach();
            }
            Ok(None) => {}
            Err(error) => self.stats.set_error(error.to_string()),
        }
        self.refresh_stats();
    }

    fn stop_proxy(&mut self) {
        self.proxy.stop(&self.stats);
        self.refresh_stats();
    }

    fn save_settings(&self) -> Result<()> {
        let mut config = self.config_snapshot();
        self.form.apply_to_config(&mut config)?;
        self.save_config_state(config)
    }

    fn save_config_state(&self, config: AppConfig) -> Result<()> {
        save_config(&self.paths, &config).context("failed to save config")?;
        *self
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = config;
        self.stats.clear_error();
        Ok(())
    }

    fn config_snapshot(&self) -> AppConfig {
        self.config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn choose_key(&mut self) {
        let current = self.form.key_path.clone();
        let mut builder = DialogBuilder::file().set_title("Choose SSH private key");
        if let Some(parent) = PathBuf::from(&current).parent().filter(|p| p.exists()) {
            builder = builder.set_location(parent);
        }
        if let Ok(Some(path)) = builder.open_single_file().show() {
            self.form.key_path = path.display().to_string();
        }
    }

    fn open_log(&mut self) {
        if let Err(error) = open::that(self.log_tail.path()) {
            self.stats
                .set_error(format!("failed to open log file: {error}"));
        }
    }

    fn reveal_log(&mut self) {
        if let Err(error) = Command::new("open")
            .arg("-R")
            .arg(self.log_tail.path())
            .spawn()
        {
            self.stats
                .set_error(format!("failed to reveal log file: {error}"));
        }
    }
}

fn main() {
    gpui::Application::new().run(|cx| {
        gpui_component::init(cx);
        settings::init_theme(cx);
        match ProxyBear::new() {
            Ok(app) => {
                let app = cx.new(|_| app);
                app.update(cx, |app, cx| app.listen(cx));
                cx.on_app_quit(move |cx| {
                    app.update(cx, |app, _| app.stop_proxy());
                    async {}
                })
                .detach();
            }
            Err(error) => {
                eprintln!("ProxyBear failed to start: {error:#}");
                cx.quit();
            }
        }
    });
}
