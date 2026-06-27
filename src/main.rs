mod config;
mod icons;
mod logging;
mod proxy;
mod settings;
mod stats;
mod tray;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::futures::{StreamExt, channel::mpsc};
use native_dialog::DialogBuilder;
use tokio::{runtime::Runtime, sync::oneshot, task::JoinHandle};
use tray_icon::menu::MenuItem;

use config::{AppConfig, AppPaths, app_paths, is_autostart_enabled, load_config, save_config};
use icons::TrayIconState;
use settings::{SettingsField, SettingsForm};
use stats::ProxyStats;
use tray::{MenuAction, TrayMenu};

const PROXY_CHANNEL_SIZE: usize = 32;
const SETTINGS_WINDOW_WIDTH: f32 = 440.0;
const SETTINGS_WINDOW_HEIGHT: f32 = 700.0;

#[cfg(target_os = "macos")]
use objc2::{class, msg_send};

#[derive(Debug, Clone)]
enum Message {
    Field(SettingsField),
    MenuAction(MenuAction),
    AutoConnect,
    ProxyDone(Option<String>),
    Tick,
    Window(iced::window::Id, iced::window::Event),
}

static PROXY_TX: Mutex<Option<mpsc::Sender<Option<String>>>> = Mutex::new(None);

struct ProxyHandle {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

struct ProxyBear {
    paths: AppPaths,
    config: Arc<Mutex<AppConfig>>,
    stats: Arc<ProxyStats>,
    runtime: Runtime,
    proxy: Option<ProxyHandle>,
    tray: TrayMenu,
    form: SettingsForm,
    stats_text: String,
    config_path: String,
    settings_window: Option<iced::window::Id>,
    last_status: String,
    last_stats: String,
    last_config: String,
    last_start_stop: String,
    last_autostart: bool,
    last_auto_connect: bool,
}

impl ProxyBear {
    fn new() -> (Self, iced::Task<Message>) {
        #[cfg(target_os = "macos")]
        unsafe {
            let ns_app: *mut objc2::runtime::AnyObject =
                msg_send![class!(NSApplication), sharedApplication];
            let _: bool = msg_send![ns_app, setActivationPolicy: 1i64];
        }
        let paths = app_paths().expect("app paths");
        logging::init(&paths.config_dir).expect("open log file");
        log::info!("ProxyBear starting");
        let config = load_config(&paths).expect("load config");
        let stats = Arc::new(ProxyStats::default());
        stats.set_status("Stopped");
        let runtime = Runtime::new().expect("tokio runtime");
        let tray = TrayMenu::new(&paths, config.auto_connect).expect("tray menu");
        let config_path = paths.config_path.display().to_string();
        let auto_connect = config.auto_connect;
        let form = SettingsForm::from_config(&config);
        let startup_task = if auto_connect {
            iced::Task::done(Message::AutoConnect)
        } else {
            iced::Task::none()
        };
        (
            Self {
                paths,
                config: Arc::new(Mutex::new(config)),
                stats,
                runtime,
                proxy: None,
                tray,
                form,
                stats_text: String::new(),
                config_path,
                settings_window: None,
                last_status: String::new(),
                last_stats: String::new(),
                last_config: String::new(),
                last_start_stop: String::new(),
                last_autostart: false,
                last_auto_connect: false,
            },
            startup_task,
        )
    }

    fn update(&mut self, msg: Message) -> iced::Task<Message> {
        match msg {
            Message::Field(f) => self.handle_field(f),
            Message::AutoConnect => self.start_proxy(),
            Message::MenuAction(a) => self.handle_menu(a),
            Message::ProxyDone(err) => {
                if self.proxy.as_ref().is_some_and(|p| p.task.is_finished()) {
                    self.proxy = None;
                }
                if let Some(e) = err {
                    self.stats.set_error(e);
                }
                self.update_icon();
                iced::Task::none()
            }
            Message::Tick => {
                if self.proxy.as_ref().is_some_and(|p| p.task.is_finished()) {
                    self.proxy = None;
                }
                self.refresh_stats();
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
            return settings::view(&self.form, &self.stats_text, &self.config_path)
                .map(Message::Field);
        }
        iced::widget::text("").into()
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        let mut subs: Vec<iced::Subscription<Message>> = vec![
            tray::subscription().map(Message::MenuAction),
            proxy_sub(),
            iced::window::events().map(|(id, ev)| Message::Window(id, ev)),
        ];
        if self.settings_window.is_some() {
            subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick));
        }
        iced::Subscription::batch(subs)
    }
}

#[derive(Hash)]
struct ProxySubId;

fn proxy_sub() -> iced::Subscription<Message> {
    iced::Subscription::run_with(ProxySubId, |_: &ProxySubId| {
        let (tx, rx) = mpsc::channel::<Option<String>>(PROXY_CHANNEL_SIZE);
        *PROXY_TX.lock().unwrap() = Some(tx);
        rx.map(Message::ProxyDone)
    })
}

impl ProxyBear {
    fn handle_menu(&mut self, action: MenuAction) -> iced::Task<Message> {
        match action {
            MenuAction::MenuOpened => {
                self.refresh_stats();
                iced::Task::none()
            }
            MenuAction::StartStop => {
                if self.proxy.is_some() {
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
            return open_task.then(iced::window::gain_focus);
        }
        iced::Task::none()
    }
}

impl ProxyBear {
    fn refresh_stats(&mut self) {
        let stats = self.stats.snapshot();
        self.stats_text = format!(
            "{} | SSH: {} ({} total) | up {} | down {}",
            stats.status,
            stats.ssh_current,
            stats.ssh_total,
            format_bytes(stats.bytes_up),
            format_bytes(stats.bytes_down)
        );
        self.update_icon();

        if !tray::is_menu_open() {
            return;
        }

        let config = self.config_snapshot();
        let running = self.proxy.is_some();

        let s = match &stats.last_error {
            Some(err) => format!("{} | Status: {}", err, stats.status),
            None => format!("Status: {}", stats.status),
        };
        set_menu_text(&self.tray.status, &mut self.last_status, s);
        let l = format!(
            "{} SSH ({} total), up {}, down {}",
            stats.ssh_current,
            stats.ssh_total,
            format_bytes(stats.bytes_up),
            format_bytes(stats.bytes_down)
        );
        set_menu_text(&self.tray.stats, &mut self.last_stats, l);
        set_menu_text(
            &self.tray.config,
            &mut self.last_config,
            config_summary(&config),
        );
        let ss = if running { "Stop Proxy" } else { "Start Proxy" };
        set_menu_text(&self.tray.start_stop, &mut self.last_start_stop, ss);
        let au = is_autostart_enabled(&self.paths);
        if au != self.last_autostart {
            self.tray.autostart.set_checked(au);
            self.last_autostart = au;
        }
        if config.auto_connect != self.last_auto_connect {
            self.tray.auto_connect.set_checked(config.auto_connect);
            self.last_auto_connect = config.auto_connect;
        }
    }

    fn update_icon(&self) {
        let running = self.proxy.is_some();
        let clean = self.stats.snapshot().last_error.is_none();
        let _ = self.tray.set_icon_state(if running && clean {
            TrayIconState::Happy
        } else {
            TrayIconState::Unhappy
        });
    }
}

impl ProxyBear {
    fn start_proxy(&mut self) -> iced::Task<Message> {
        if self.proxy.is_some() {
            return iced::Task::none();
        }
        if let Err(e) = self.config.lock().unwrap().validate_ready() {
            self.stats.set_error(e.to_string());
            return iced::Task::none();
        }
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let config = Arc::clone(&self.config);
        let paths = self.paths.clone();
        let stats = Arc::clone(&self.stats);
        stats.set_status("Starting");
        stats.clear_error();
        let task = self.runtime.spawn(async move {
            let r = proxy::run_proxy(config, paths, Arc::clone(&stats), shutdown_rx).await;
            stats.set_status("Stopped");
            if let Some(tx) = PROXY_TX.lock().unwrap().as_mut() {
                let _ = tx.try_send(r.err().map(|e| e.to_string()));
            }
        });
        self.proxy = Some(ProxyHandle {
            shutdown: Some(shutdown_tx),
            task,
        });
        self.update_icon();
        iced::Task::none()
    }

    fn stop_proxy(&mut self) {
        if let Some(mut p) = self.proxy.take() {
            if let Some(s) = p.shutdown.take() {
                let _ = s.send(());
            }
            p.task.abort();
        }
        self.stats.set_status("Stopped");
        self.update_icon();
    }

    fn save_settings(&self) {
        let mut config = self.config_snapshot();
        self.form.apply_to_config(&mut config);
        self.save_config_state(config);
    }

    fn save_config_state(&self, config: AppConfig) {
        let _ = save_config(&self.paths, &config);
        *self.config.lock().unwrap() = config;
    }

    fn config_snapshot(&self) -> AppConfig {
        self.config.lock().unwrap().clone()
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
    const U: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < U.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", U[unit])
    }
}

fn main() -> iced::Result {
    iced::daemon(ProxyBear::new, ProxyBear::update, ProxyBear::view)
        .title("ProxyBear")
        .subscription(ProxyBear::subscription)
        .run()
}
