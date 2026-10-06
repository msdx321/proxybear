# ProxyBear

A native macOS menu-bar app that runs a local SOCKS5 proxy over SSH.

## Features

- **Menu-bar only**: no Dock icon, lives in the menu bar with a bear tray icon
- **SOCKS5 over SSH**: tunnels your traffic through an SSH server
- **No local authentication**: local proxy is unauthenticated, for use by local tools
- **Launch at login**: optional LaunchAgent for autostart
- **GPUI settings**: grouped connection settings, masked credentials, validation feedback, and searchable activity logs
- **Keychain secrets**: SSH passwords and key passphrases are stored in the macOS Keychain
- **Host key verification**: asks you to confirm a server's key on first connect and when it changes
- **Automatic updates**: checks for new releases with Sparkle

## Installation

### Homebrew

```sh
brew install --cask msdx321/tap/proxybear
```

ProxyBear requires Apple Silicon and macOS 14 Sonoma or later.
Run `brew update` followed by `brew upgrade --cask msdx321/tap/proxybear` to update.

### DMG

1. Download `ProxyBear-<version>.dmg` from the [latest release](https://github.com/msdx321/proxybear/releases/latest) and open it.
2. Drag **ProxyBear** onto the **Applications** shortcut in the installer window.
3. Eject **Install ProxyBear**, then open **ProxyBear** from Applications. Look for the bear icon in the menu bar; the app has no Dock icon.

> [!IMPORTANT]
> Because ProxyBear is not notarized by Apple, macOS Gatekeeper may block it on first launch.
> After trying to open it, go to **System Settings → Privacy & Security → Open Anyway** and confirm.
> See [Apple's first-launch instructions](https://support.apple.com/en-us/102445).

### From source

```sh
cargo install cargo-bundle
cargo bundle --release
open target/release/bundle/osx/ProxyBear.app
```

## Usage

1. Click the bear icon in the menu bar
2. Choose **Settings…**
3. On the **Connection** page, fill in your SSH server, username, and private key path
4. Click **Save and Start**

The proxy listens on `127.0.0.1:1080` by default. Point your browser or tools at `socks5://127.0.0.1:1080`.

## License

[MIT](LICENSE)
