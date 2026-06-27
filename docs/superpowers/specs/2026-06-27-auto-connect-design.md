# Auto-Connect on App Open

**Date:** 2026-06-27
**Status:** approved

## Summary

Add an `auto_connect` config option. When enabled, the SOCKS5 proxy starts automatically when the app opens — no manual "Start Proxy" click needed.

## Changes

### 1. Config (`src/config.rs`)
- Add `auto_connect: bool` to `AppConfig`, default `false`
- Serialized as `auto_connect = false` in `config.toml`

### 2. Tray Menu (`src/main.rs`)
- New `"Auto-Connect"` checkbox menu item below `"Launch at Login"`
- New `MenuAction::ToggleAutoConnect`
- Toggles field in config, saves to disk

### 3. Settings Window (`src/settings.rs` + `src/main.rs`)
- Add `auto_connect` checkbox to settings form
- Saved with other fields on "Save and Start"

### 4. Startup Trigger (`src/main.rs`)
- `ProxyBear::new()`: if `auto_connect`, return `Task` dispatching `Message::AutoConnect`
- `update()`: `Message::AutoConnect` calls existing `start_proxy()`
- Existing guard prevents double-starts
- Invalid config → status error (same as manual start)

## Scope
No new proxy logic, no new validation. Reuses all existing `start_proxy()` machinery.
