<div align="center">

# Avari Mieru

**Your Mieru VPN, in one app.**

macOS · Linux · Android

[Русский](README.md) · English

[Downloads](https://github.com/OstKost/avari-meiru/releases) · [Server setup](#server-setup) · [Release policy](RELEASES.md) · [Issues](https://github.com/OstKost/avari-meiru/issues)

</div>

Avari Mieru is an MVP client for a self-hosted Mieru server. It brings profile management, connection controls, status and sanitized logs together in a desktop or Android interface. The desktop app uses Tauri, Rust and the pinned [mbox](https://github.com/enfein/mbox) core; Android uses Kotlin, Compose and VpnService.

## Platforms

- **macOS:** Apple Silicon debug app tested; Intel GUI has not been tested. macOS requests administrator privileges for the network process.
- **Linux:** x86_64 packages target Ubuntu 24.04, Debian 13 and Arch. Native desktop acceptance testing has been verified in a real Ubuntu 24.04 X11/Xfce session with polkit-gnome and live VPS traffic ([report](packaging/linux/ACCEPTANCE.md)). Server administration via `mieru-admin` is verified under systemd ([report](server/SMOKE.md)).
- **Android:** API 26+, arm64-v8a and x86_64 debug APK. A connection was tested on an emulator; physical devices and API 26 remain unverified.
- **Windows and iOS:** deferred.

A running Mieru server and valid credentials are required. Example profiles contain fictional credentials. The connected status indicates local TUN readiness, not proof that the server is reachable or the public IP has changed.

## Interfaces

Client screenshots are not yet available. The desktop and Android interfaces provide profile and connection controls. The server also includes a terminal interface; the following is an illustrative excerpt with fictional data:

```text
 Mieru Admin TUI  |  Управление сервером Mieru (mita)

 • ОС: Ubuntu (x86_64, 24.04)
 • Пакет mita: Установлен (3.38.0)
 • Systemd служба: active (running)
 • Mita proxy: RUNNING
 • Основной TCP порт: 28443
 • Пользователей в системе: 1

 ↑/↓: select  •  Enter: open  •  q/Esc: exit
```

The TUI currently uses Russian labels. Its setup wizard asks for the server address, TCP port, username and password, then previews the configuration before applying it.

## Server setup

On an x86_64 Ubuntu 24.04 or Debian 13 VPS with systemd, Go and network access to dependencies:

```sh
git clone https://github.com/OstKost/avari-meiru.git
cd avari-meiru/server
CGO_ENABLED=0 go build -trimpath -o mieru-admin ./cmd/mieru-admin
sudo ./mieru-admin setup
```

Setup installs the manager and its service and opens the TUI. Install `mita` using the [manual guide](docs/VPS_SETUP.md), then configure it with the wizard; allow the selected TCP port through the VPS firewall. Enter the server address, port and credentials in the client. To reopen the TUI, run `sudo mieru-admin tui`. The management API binds to localhost only. See [server documentation](server/README.md) and the [manual VPS guide](docs/VPS_SETUP.md).

## Build from source

For macOS, install Rust, Xcode Command Line Tools, Go, Node.js and pnpm, then:

```sh
./core/build-desktop.sh
pnpm --dir desktop/ui install --frozen-lockfile
cd desktop
ui/node_modules/.bin/tauri build --debug --bundles app
```

See [desktop](desktop/README.md), [Android](android/README.md), [Linux packaging](packaging/linux/README.md), and [core](core/README.md) for platform-specific instructions. Debug builds are not signed release installers. The macOS profile password is stored locally without encryption, with file permissions `0600`.

## Verification and limitations

Android was tested against a temporary server on an emulator; Linux GUI and helper were verified both with a local server under Xvfb and in full native X11/polkit acceptance testing ([packaging/linux/ACCEPTANCE.md](packaging/linux/ACCEPTANCE.md)); macOS Apple Silicon was tested with a temporary remote server. Server manager `mieru-admin` was verified under genuine systemd PID 1 ([server/SMOKE.md](server/SMOKE.md)). These tests do not establish compatibility with every device or network. See [macOS](docs/MACOS_SMOKE.md), [Linux Acceptance](packaging/linux/ACCEPTANCE.md), [Linux Smoke](docs/LINUX_SMOKE.md), [Android](android/SMOKE.md), and [server](server/SMOKE.md) reports. The pinned mbox revision is `1c5b39718ebcd05e6adb31f95246fbf056be9bd7`.
