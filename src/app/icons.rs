use anyhow::{Context, Result};
use tray_icon::Icon;

const ICON_SIZE: u32 = 32;
const HAPPY_ICON_RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/happy-tray-icon.rgba"));
const UNHAPPY_ICON_RGBA: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/unhappy-tray-icon.rgba"));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayIconState {
    Happy,
    Unhappy,
}

pub fn tray_icon(state: TrayIconState) -> Result<Icon> {
    let rgba = match state {
        TrayIconState::Happy => HAPPY_ICON_RGBA,
        TrayIconState::Unhappy => UNHAPPY_ICON_RGBA,
    };
    Icon::from_rgba(rgba.to_vec(), ICON_SIZE, ICON_SIZE).context("failed to create tray icon")
}
