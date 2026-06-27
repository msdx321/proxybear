# ProxyBear

A native macOS menu-bar app that runs a local SOCKS5 proxy over SSH.

## Features

- **Menu-bar only**: no Dock icon, lives in the menu bar with a bear tray icon
- **SOCKS5 over SSH**: tunnels your traffic through an SSH server
- **No local authentication**: local proxy is unauthenticated, for use by local tools
- **Launch at login**: optional LaunchAgent for autostart
- **Settings UI**: configure server, authentication, and bind address in the settings window

## Installation

Download `ProxyBear.dmg` from the [latest release](https://github.com/msdx321/proxybear/releases/latest), open it, and drag ProxyBear to your Applications folder.

> [!IMPORTANT]
> Because ProxyBear is not notarized by Apple, macOS Gatekeeper may block it on first launch.
> **Right-click** the app in Finder and choose **Open**, then click **Open** in the dialog.
> Alternatively, run `xattr -cr /Applications/ProxyBear.app` in Terminal to clear the quarantine flag.

### From source

```sh
cargo install cargo-bundle
cargo bundle --release
open target/release/bundle/osx/ProxyBear.app
```

## Usage

1. Click the bear icon in the menu bar
2. Choose **Settings…**
3. Fill in your SSH server, username, and private key path
4. Click **Save and Start**

The proxy listens on `127.0.0.1:1080` by default. Point your browser or tools at `socks5://127.0.0.1:1080`.

## License

[MIT](LICENSE)
