// allow: SIZE_OK - main owns the Iced daemon state and message routing by user preference.
mod app;
mod config;
mod proxy;
mod settings;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use native_dialog::DialogBuilder;

use app::{
    logging, platform, presentation,
    presentation::MenuPresenter,
    proxy_control::{self, ProxyController, ProxyEvent},
    stats::ProxyStats,
    tray::{self, MenuAction, TrayMenu},
};
use config::{AppConfig, AppPaths, app_paths, load_config, save_config};
use settings::{LogTail, SettingsField, SettingsForm, SettingsTab};

const SETTINGS_WINDOW_WIDTH: f32 = 520.0;
const SETTINGS_WINDOW_HEIGHT: f32 = 640.0;

#[derive(Debug, Clone)]
enum Message {
    Field(SettingsField),
    MenuAction(MenuAction),
    AutoConnect,
    Proxy(ProxyEvent),
    Tick,
    LogTick,
    Window(iced::window::Id, iced::window::Event),
}

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
    settings_window: Option<iced::window::Id>,
}

impl ProxyBear {
    fn new() -> (Self, iced::Task<Message>) {
        match Self::try_new() {
            Ok(app) => app,
            Err(error) => {
                eprintln!("ProxyBear failed to start: {error:?}");
                std::process::exit(1);
            }
        }
    }

    fn try_new() -> Result<(Self, iced::Task<Message>)> {
        platform::activate_as_accessory();
        let paths = app_paths().context("app paths")?;
        logging::init(&paths.config_dir).context("open log file")?;
        log::info!("ProxyBear starting");
        let config = load_config(&paths).context("load config")?;
        let stats = Arc::new(ProxyStats::default());
        stats.set_status("Stopped");
        let proxy = ProxyController::new().context("create proxy controller")?;
        let tray = TrayMenu::new(&paths, config.auto_connect).context("tray menu")?;
        let config_path = paths.config_path.display().to_string();
        let auto_connect = config.auto_connect;
        let form = SettingsForm::from_config(&config);
        let log_tail = LogTail::new(paths.log_path());
        let startup_task = if auto_connect {
            iced::Task::done(Message::AutoConnect)
        } else {
            iced::Task::none()
        };
        Ok((
            Self {
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
                settings_window: None,
            },
            startup_task,
        ))
    }

    fn update(&mut self, msg: Message) -> iced::Task<Message> {
        match msg {
            Message::Field(f) => self.handle_field(f),
            Message::AutoConnect => self.start_proxy(),
            Message::MenuAction(a) => self.handle_menu(a),
            Message::Proxy(event) => self.handle_proxy_event(event),
            Message::Tick => {
                self.proxy.reap_finished();
                self.refresh_stats();
                iced::Task::none()
            }
            Message::LogTick => {
                if self.active_tab == SettingsTab::Logs {
                    self.log_tail.refresh();
                }
                iced::Task::none()
            }
            Message::Window(id, ev) => {
                if matches!(ev, iced::window::Event::Closed) && self.settings_window == Some(id) {
                    self.settings_window = None;
                }
                iced::Task::none()
            }
        }
    }

    fn view(&self, window: iced::window::Id) -> iced::Element<'_, Message> {
        if Some(window) == self.settings_window {
            return settings::view(
                &self.form,
                self.active_tab,
                &self.log_tail,
                &self.stats_text,
                &self.config_path,
            )
            .map(Message::Field);
        }
        iced::widget::text("").into()
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        let mut subs: Vec<iced::Subscription<Message>> = vec![
            tray::subscription().map(Message::MenuAction),
            proxy_control::subscription().map(Message::Proxy),
            iced::window::events().map(|(id, ev)| Message::Window(id, ev)),
        ];
        if self.settings_window.is_some() {
            subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick));
        }
        if self.settings_window.is_some() && self.active_tab == SettingsTab::Logs {
            subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::LogTick));
        }
        iced::Subscription::batch(subs)
    }
}

impl ProxyBear {
    fn handle_menu(&mut self, action: MenuAction) -> iced::Task<Message> {
        match action {
            MenuAction::MenuOpened => {
                self.refresh_stats();
                iced::Task::none()
            }
            MenuAction::StartStop => {
                if self.proxy.is_running() {
                    self.stop_proxy();
                    iced::Task::none()
                } else {
                    self.start_proxy()
                }
            }
            MenuAction::Settings => self.toggle_settings(),
            MenuAction::ToggleAutostart => {
                let mut config = self.config_snapshot();
                config.autostart = !config.autostart;
                self.tray.autostart.set_checked(config.autostart);
                let _ = config::set_autostart(&self.paths, config.autostart);
                self.save_config_state(config);
                iced::Task::none()
            }
            MenuAction::ToggleAutoConnect => {
                let mut config = self.config_snapshot();
                config.auto_connect = !config.auto_connect;
                self.tray.auto_connect.set_checked(config.auto_connect);
                self.save_config_state(config);
                iced::Task::none()
            }
            MenuAction::Quit => {
                self.stop_proxy();
                std::process::exit(0);
            }
        }
    }

    fn handle_field(&mut self, field: SettingsField) -> iced::Task<Message> {
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
            SettingsField::Save => {
                self.save_settings();
            }
            SettingsField::SaveAndStart => {
                self.save_settings();
                return self.start_proxy();
            }
            SettingsField::Stop => {
                self.stop_proxy();
            }
            SettingsField::ChooseKey => {
                self.save_settings();
                self.choose_key();
            }
        }
        iced::Task::none()
    }

    fn toggle_settings(&mut self) -> iced::Task<Message> {
        if self.settings_window.is_some() {
            if let Some(id) = self.settings_window.take() {
                return iced::window::close(id);
            }
        } else {
            let (id, open_task) = iced::window::open(iced::window::Settings {
                size: iced::Size::new(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT),
                ..Default::default()
            });
            self.settings_window = Some(id);
            self.refresh_stats();
            if self.active_tab == SettingsTab::Logs {
                self.log_tail.refresh();
            }
            return open_task.then(iced::window::gain_focus);
        }
        iced::Task::none()
    }
}

impl ProxyBear {
    fn refresh_stats(&mut self) {
        let stats = self.stats.snapshot();
        self.stats_text = presentation::settings_status(&stats);
        self.update_icon();

        let config = self.config_snapshot();
        self.menu.update_tray(
            &self.tray,
            &self.paths,
            &config,
            &stats,
            self.proxy.is_running(),
        );
    }

    fn update_icon(&self) {
        let clean = self.stats.snapshot().last_error.is_none();
        let _ = self
            .tray
            .set_icon_state(presentation::icon_state(self.proxy.is_running(), clean));
    }
}

impl ProxyBear {
    fn start_proxy(&mut self) -> iced::Task<Message> {
        if let Err(error) = self.proxy.start(
            Arc::clone(&self.config),
            self.paths.clone(),
            Arc::clone(&self.stats),
        ) {
            self.stats.set_error(error.to_string());
        }
        self.update_icon();
        iced::Task::none()
    }

    fn stop_proxy(&mut self) {
        self.proxy.stop(&self.stats);
        self.update_icon();
    }

    fn handle_proxy_event(&mut self, event: ProxyEvent) -> iced::Task<Message> {
        match event {
            ProxyEvent::Done(error) => {
                self.proxy.reap_finished();
                if let Some(error) = error {
                    self.stats.set_error(error);
                }
                self.update_icon();
                iced::Task::none()
            }
        }
    }

    fn save_settings(&self) {
        let mut config = self.config_snapshot();
        self.form.apply_to_config(&mut config);
        self.save_config_state(config);
    }

    fn save_config_state(&self, config: AppConfig) {
        if let Err(error) = save_config(&self.paths, &config) {
            self.stats
                .set_error(format!("failed to save config: {error}"));
            return;
        }
        *self
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = config;
    }

    fn config_snapshot(&self) -> AppConfig {
        self.config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn choose_key(&mut self) {
        let current = self.config_snapshot().key_path;
        let mut builder = DialogBuilder::file().set_title("Choose SSH private key");
        if let Some(parent) = PathBuf::from(&current).parent().filter(|p| p.exists()) {
            builder = builder.set_location(parent);
        }
        if let Ok(Some(path)) = builder.open_single_file().show() {
            self.form.key_path = path.display().to_string();
        }
    }
}

fn main() -> iced::Result {
    iced::daemon(ProxyBear::new, ProxyBear::update, ProxyBear::view)
        .title("ProxyBear")
        .subscription(ProxyBear::subscription)
        .run()
}
