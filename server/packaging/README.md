# Mieru Admin (mieru-admin)

API-first daemon and BubbleTea TUI for managing Mieru (mita v3.38.0) proxy servers on Ubuntu 24.04 and Debian 13 (x86_64).

## Architecture

- **Daemon**: Root management service running on `127.0.0.1:9081/v1` under systemd.
- **Authentication**: Cryptographically secure token stored in `/etc/mieru-admin/token` (`0600` permissions), transmitted via `Authorization: Bearer <token>`.
- **TUI**: Terminal user interface built with BubbleTea (pinned v1.3.10) communicating strictly through the HTTP API.
- **Security**: No public binding, no CORS, no plaintext password storage. Passwords hashed with official mita algorithm. Secret sanitization across all logs and error messages. Optimistic locking with revision diffing and automatic rollback on failure.

## Installation & Setup

1. Extract the release archive:
   ```bash
   tar -xzf mieru-admin-linux-amd64.tar.gz
   cd mieru-admin-linux-amd64
   ```

2. Run automated setup (as root):
   ```bash
   sudo ./mieru-admin setup
   ```
   This will:
   - Install binary to `/usr/local/bin/mieru-admin` (0755)
   - Generate `/etc/mieru-admin/token` (0600)
   - Install and start `/etc/systemd/system/mieru-admin.service`
   - Launch the interactive Russian-language TUI

3. Non-interactive setup (for scripts/automation):
   ```bash
   sudo ./mieru-admin setup --no-tui
   ```

## CLI Commands

- `mieru-admin setup` — Install daemon binary, token, systemd service, and enter TUI.
- `mieru-admin serve` — Run HTTP API daemon on `127.0.0.1:9081` (under systemd).
- `mieru-admin tui` — Launch interactive BubbleTea terminal UI.
- `mieru-admin version` — Display build and version info.

## Remote Desktop Access via SSH Tunnel

For future desktop clients or remote administration:
```bash
ssh -L 9081:127.0.0.1:9081 root@<your-vps-ip>
```
The desktop client connects locally to `http://127.0.0.1:9081/v1` using the Bearer token retrieved from `/etc/mieru-admin/token`.
