use std::{
    cell::Cell,
    sync::{Mutex, MutexGuard},
};

use anyhow::{Context, Result};
use iced::futures::channel::mpsc;
use tray_icon::{
    TrayIcon, TrayIconBuilder,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

use crate::config::{AppPaths, is_autostart_enabled};

use super::icons::{self, TrayIconState};

const MENU_CHANNEL_SIZE: usize = 32;

#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::{AnyObject, NSObject};
#[cfg(target_os = "macos")]
use objc2::{ClassType, define_class, msg_send};

#[derive(Debug, Clone)]
pub enum MenuAction {
    StartStop,
    Settings,
    ToggleAutostart,
    ToggleAutoConnect,
    Quit,
    MenuOpened,
}

static MENU_TX: Mutex<Option<mpsc::Sender<MenuAction>>> = Mutex::new(None);

#[cfg(target_os = "macos")]
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "ProxyBearMenuDelegate"]
    pub struct MenuDelegate;

    impl MenuDelegate {
        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open(&self, _menu: &AnyObject) {
            if let Some(tx) = menu_sender().as_mut() {
                let _ = tx.try_send(MenuAction::MenuOpened);
            }
        }
    }
);

#[derive(Hash)]
struct MenuSubId;

pub fn subscription() -> iced::Subscription<MenuAction> {
    iced::Subscription::run_with(MenuSubId, |_: &MenuSubId| {
        let (tx, rx) = mpsc::channel::<MenuAction>(MENU_CHANNEL_SIZE);
        *menu_sender() = Some(tx);
        rx
    })
}

pub struct TrayMenu {
    _tray: TrayIcon,
    #[cfg(target_os = "macos")]
    _delegate: Retained<MenuDelegate>,
    icon_state: Cell<TrayIconState>,
    pub status: MenuItem,
    pub stats: MenuItem,
    pub config: MenuItem,
    pub start_stop: MenuItem,
    pub autostart: CheckMenuItem,
    pub auto_connect: CheckMenuItem,
}

impl TrayMenu {
    pub fn new(paths: &AppPaths, auto_connect: bool) -> Result<Self> {
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
            let action = match event.id.as_ref() {
                "start_stop" => Some(MenuAction::StartStop),
                "settings" => Some(MenuAction::Settings),
                "autostart" => Some(MenuAction::ToggleAutostart),
                "auto_connect" => Some(MenuAction::ToggleAutoConnect),
                "quit" => Some(MenuAction::Quit),
                _ => None,
            };
            if let (Some(action), Some(tx)) = (action, menu_sender().as_mut()) {
                let _ = tx.try_send(action);
            }
        }));

        Ok(Self {
            _tray: tray,
            icon_state: Cell::new(icon_state),
            status,
            stats,
            config,
            start_stop,
            autostart,
            auto_connect,
            #[cfg(target_os = "macos")]
            _delegate: delegate,
        })
    }

    pub fn set_icon_state(&self, state: TrayIconState) -> Result<()> {
        if self.icon_state.get() == state {
            return Ok(());
        }
        self._tray
            .set_icon_with_as_template(Some(icons::tray_icon(state)?), true)?;
        self.icon_state.set(state);
        Ok(())
    }
}

fn menu_sender() -> MutexGuard<'static, Option<mpsc::Sender<MenuAction>>> {
    MENU_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
