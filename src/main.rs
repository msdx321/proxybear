mod config;
mod icons;
mod proxy;
mod settings;
mod stats;

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use config::{AppConfig, AppPaths, app_paths, is_autostart_enabled, load_config, save_config};
use iced::futures::{StreamExt, channel::mpsc};
use icons::TrayIconState;
use native_dialog::DialogBuilder;
use stats::ProxyStats;
use tokio::{runtime::Runtime, sync::oneshot, task::JoinHandle};
use tray_icon::{
    TrayIcon, TrayIconBuilder,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::{AnyObject, NSObject};
#[cfg(target_os = "macos")]
use objc2::{ClassType, class, define_class, msg_send};

#[derive(Debug, Clone)]
enum MenuAction {
    StartStop,
    Settings,
    ToggleAutostart,
    ToggleAutoConnect,
    Quit,
    MenuOpened,
}

#[derive(Debug, Clone)]
pub enum SettingsField {
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

#[derive(Debug, Clone)]
enum Message {
    Field(SettingsField),
    MenuAction(MenuAction),
    AutoConnect,
    ProxyDone(Option<String>),
    Tick,
    Window(iced::window::Id, iced::window::Event),
}

static MENU_TX: Mutex<Option<mpsc::Sender<MenuAction>>> = Mutex::new(None);
static PROXY_TX: Mutex<Option<mpsc::Sender<Option<String>>>> = Mutex::new(None);

#[cfg(target_os = "macos")]
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "ProxyBearMenuDelegate"]
    pub struct MenuDelegate;

    impl MenuDelegate {
        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open(&self, _menu: &AnyObject) {
            MENU_IS_OPEN.with(|c| c.set(true));
            if let Some(tx) = MENU_TX.lock().unwrap().as_mut() {
                let _ = tx.try_send(MenuAction::MenuOpened);
            }
        }
        #[unsafe(method(menuDidClose:))]
        fn menu_did_close(&self, _menu: &AnyObject) {
            MENU_IS_OPEN.with(|c| c.set(false));
        }
    }
);

#[cfg(target_os = "macos")]
thread_local! { static MENU_IS_OPEN: Cell<bool> = Cell::new(false); }

struct ProxyHandle {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

struct TrayMenu {
    tray: TrayIcon,
    #[cfg(target_os = "macos")]
    _delegate: Retained<MenuDelegate>,
    icon_state: Cell<TrayIconState>,
    status: MenuItem,
    stats: MenuItem,
    config: MenuItem,
    start_stop: MenuItem,
    autostart: CheckMenuItem,
    auto_connect: CheckMenuItem,
}

impl TrayMenu {
    fn new(paths: &AppPaths, auto_connect: bool) -> Result<Self> {
        let menu = Menu::new();
        let status = MenuItem::with_id("status", "Status: Stopped", false, None);
        let stats = MenuItem::with_id("stats", "0 connections", false, None);
        let config = MenuItem::with_id("config", "No server configured", false, None);
        let start_stop = MenuItem::with_id("start_stop", "Start Proxy", true, None);
        let settings = MenuItem::with_id("settings", "Settings\u{2026}", true, None);
        let autostart = CheckMenuItem::with_id(
            "autostart",
            "Launch at Login",
            true,
            is_autostart_enabled(paths),
            None,
        );
        let auto_connect =
            CheckMenuItem::with_id("auto_connect", "Auto-Connect", true, auto_connect, None);
        let quit = MenuItem::with_id("quit", "Quit", true, None);
        let sep = PredefinedMenuItem::separator();
        menu.append_items(&[
            &status,
            &stats,
            &config,
            &sep,
            &start_stop,
            &settings,
            &autostart,
            &auto_connect,
            &sep,
            &quit,
        ])?;

        let icon_state = TrayIconState::Unhappy;
        let tray = TrayIconBuilder::new()
            .with_icon(icons::tray_icon(icon_state)?)
            .with_icon_as_template(true)
            .with_tooltip("ProxyBear")
            .with_menu(Box::new(menu))
            .build()
            .context("failed to create menu bar item")?;

        #[cfg(target_os = "macos")]
        let delegate: Retained<MenuDelegate> = unsafe { msg_send![MenuDelegate::class(), new] };
        #[cfg(target_os = "macos")]
        if let Some(si) = tray.ns_status_item() {
            unsafe {
                let m: Retained<AnyObject> = msg_send![&si, menu];
                let _: () = msg_send![&m, setDelegate: &*delegate];
            }
        }

        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let a = match event.id.as_ref() {
                "start_stop" => Some(MenuAction::StartStop),
                "settings" => Some(MenuAction::Settings),
                "autostart" => Some(MenuAction::ToggleAutostart),
                "auto_connect" => Some(MenuAction::ToggleAutoConnect),
                "quit" => Some(MenuAction::Quit),
                _ => None,
            };
            if let (Some(a), Some(tx)) = (a, MENU_TX.lock().unwrap().as_mut()) {
                let _ = tx.try_send(a);
            }
        }));

        Ok(Self {
            tray,
            icon_state: Cell::new(icon_state),
            status,
            stats,
            config,
            start_stop,
            autostart,
            auto_connect,
            _delegate: delegate,
        })
    }

    fn set_icon_state(&self, state: TrayIconState) -> Result<()> {
        if self.icon_state.get() == state {
            return Ok(());
        }
        self.tray
            .set_icon_with_as_template(Some(icons::tray_icon(state)?), true)?;
        self.icon_state.set(state);
        Ok(())
    }
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
    settings_open: bool,
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
        let config = load_config(&paths).expect("load config");
        let stats = Arc::new(ProxyStats::default());
        stats.set_status("Stopped");
        let runtime = Runtime::new().expect("tokio runtime");
        let tray = TrayMenu::new(&paths, config.auto_connect).expect("tray menu");
        let config_path = paths.config_path.display().to_string();
        let auto_connect = config.auto_connect;
        let form = SettingsForm {
            server: config.server.clone(),
            username: config.username.clone(),
            port: config.port.to_string(),
            auth_method: config.auth_method.clone(),
            key_path: config.key_path.clone(),
            key_password: config.key_password.clone(),
            ssh_password: config.ssh_password.clone(),
            local_addr: config.local_addr.clone(),
        };
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
                settings_open: false,
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
                    self.settings_open = false;
                    self.settings_window = None;
                }
                iced::Task::none()
            }
        }
    }

    fn view(&self, window: iced::window::Id) -> iced::Element<'_, Message> {
        if Some(window) == self.settings_window && self.settings_open {
            return settings::view(&self.form, &self.stats_text, &self.config_path)
                .map(Message::Field);
        }
        iced::widget::text("").into()
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        let mut subs: Vec<iced::Subscription<Message>> = vec![
            menu_sub(),
            proxy_sub(),
            iced::window::events().map(|(id, ev)| Message::Window(id, ev)),
        ];
        if self.settings_open {
            subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick));
        }
        iced::Subscription::batch(subs)
    }
}

#[derive(Hash)]
struct MenuSubId;
#[derive(Hash)]
struct ProxySubId;

fn menu_sub() -> iced::Subscription<Message> {
    iced::Subscription::run_with(MenuSubId, |_: &MenuSubId| {
        let (tx, rx) = mpsc::channel::<MenuAction>(32);
        *MENU_TX.lock().unwrap() = Some(tx);
        rx.map(Message::MenuAction)
    })
}

fn proxy_sub() -> iced::Subscription<Message> {
    iced::Subscription::run_with(ProxySubId, |_: &ProxySubId| {
        let (tx, rx) = mpsc::channel::<Option<String>>(32);
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
                let mut config = self.config.lock().unwrap().clone();
                config.autostart = !config.autostart;
                self.tray.autostart.set_checked(config.autostart);
                let _ = config::set_autostart(&self.paths, config.autostart);
                let _ = save_config(&self.paths, &config);
                *self.config.lock().unwrap() = config;
                iced::Task::none()
            }
            MenuAction::ToggleAutoConnect => {
                let mut config = self.config.lock().unwrap().clone();
                config.auto_connect = !config.auto_connect;
                self.tray.auto_connect.set_checked(config.auto_connect);
                let _ = save_config(&self.paths, &config);
                *self.config.lock().unwrap() = config;
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
        if self.settings_open {
            self.settings_open = false;
            if let Some(id) = self.settings_window.take() {
                return iced::window::close(id);
            }
        } else {
            let (id, open_task) = iced::window::open(iced::window::Settings {
                size: iced::Size::new(440.0, 520.0),
                ..Default::default()
            });
            self.settings_open = true;
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
            "{} | {} active | up {} | down {}",
            stats.status,
            stats.active_connections,
            format_bytes(stats.bytes_up),
            format_bytes(stats.bytes_down)
        );
        self.config_path = self.paths.config_path.display().to_string();

        #[cfg(target_os = "macos")]
        let visible = MENU_IS_OPEN.with(|c| c.get());
        #[cfg(not(target_os = "macos"))]
        let visible = false;
        if !visible {
            return;
        }

        let config = self.config.lock().unwrap().clone();
        let running = self.proxy.is_some();

        let s = format!("Status: {}", stats.status);
        if s != self.last_status {
            self.tray.status.set_text(&s);
            self.last_status = s;
        }
        let l = format!(
            "{} total, {} active, up {}, down {}",
            stats.total_connections,
            stats.active_connections,
            format_bytes(stats.bytes_up),
            format_bytes(stats.bytes_down)
        );
        if l != self.last_stats {
            self.tray.stats.set_text(&l);
            self.last_stats = l;
        }
        let c = config_summary(&config);
        if c != self.last_config {
            self.tray.config.set_text(&c);
            self.last_config = c;
        }
        let ss = if running { "Stop Proxy" } else { "Start Proxy" };
        if ss != self.last_start_stop {
            self.tray.start_stop.set_text(ss);
            self.last_start_stop = ss.to_string();
        }
        let au = is_autostart_enabled(&self.paths);
        if au != self.last_autostart {
            self.tray.autostart.set_checked(au);
            self.last_autostart = au;
        }
        let ac = self.config.lock().unwrap().auto_connect;
        if ac != self.last_auto_connect {
            self.tray.auto_connect.set_checked(ac);
            self.last_auto_connect = ac;
        }
    }

    fn update_icon(&self) {
        let _ = self.tray.set_icon_state(if self.proxy.is_some() {
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
        let mut config = self.config.lock().unwrap().clone();
        let f = &self.form;
        config.server = f.server.trim().to_string();
        config.username = f.username.trim().to_string();
        config.port = f.port.parse().unwrap_or(22);
        config.auth_method = if f.auth_method == "password" {
            "password".into()
        } else {
            "key".into()
        };
        config.key_path = f.key_path.trim().to_string();
        config.key_password = f.key_password.clone();
        config.ssh_password = f.ssh_password.clone();
        config.local_addr = f.local_addr.trim().to_string();
        let _ = save_config(&self.paths, &config);
        *self.config.lock().unwrap() = config;
    }

    fn choose_key(&mut self) {
        let current = self.config.lock().unwrap().key_path.clone();
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
