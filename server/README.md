# Mieru Admin (`mieru-admin`)

API-first management daemon and interactive BubbleTea terminal user interface (TUI) for Mieru proxy server (`mita` v3.38.0). Designed for Ubuntu 24.04 and Debian 13 (x86_64) servers with systemd.

## Key Design Principles & Security Model

1. **Independent Module**: Isolated Go module in `server/` with zero dependencies on other subdirectories.
2. **API-First Architecture**: Business logic resides strictly in `domain` and `service` layers. The BubbleTea TUI communicates exclusively via the localhost HTTP API client (`http.Client`).
3. **No Secret Leakage**:
   - Authentication token is generated cryptographically (32 random bytes) and stored at `/etc/mieru-admin/token` with strict `0600` permissions, root ownership, and rejection of symlinks.
   - Token is never passed via CLI argv or logged. Transmitted solely via HTTP `Authorization: Bearer <token>` header.
   - Passwords are never stored in plaintext and never leaked in request logs, error responses, or screen output.
   - Password hashes use the official mita cryptographic algorithm (`sha256(password + 0x00 + username)`).
4. **Authoritative State & Zero-Loss Rollback**:
   - `ConfigManager` automatically discovers existing configurations from native `/etc/mita/server.conf.pb` if human JSON is absent, preventing accidental loss of pre-existing users or port bindings.
   - Dual-file snapshot (`server_config.json` and `server.conf.pb`) preserves original file ownership, permissions, and runtime proxy state (`RUNNING` vs `STOPPED`).
   - If `mita replace config` or service reload fails, both configuration files and previous runtime service states are atomically rolled back.
   - Optimistic concurrency control (revision SHA256) rejects concurrent edits with `409 Conflict`.
5. **Single Binary & Systemd Service**:
   - Single static Linux binary `mieru-admin` (`CGO_ENABLED=0`, stripped, reproducible build).
   - Dedicated `mieru-admin.service` systemd unit under root management. Does not duplicate or compete with native `mita.service`.

---

## Build Instructions

Reproducible compilation for Linux amd64:
```bash
cd server
GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build -trimpath -ldflags="-s -w -buildid=" -o dist/mieru-admin ./cmd/mieru-admin
```

To run all unit tests:
```bash
go test -v ./...
```

To run live pseudo-terminal (PTY) 80x24 verification:
```bash
python3 test/live_pty_test.py
```

---

## CLI Usage

### 1. `setup` (Root Only)
Installs binary to `/usr/local/bin/mieru-admin`, generates token `0600`, installs `mieru-admin.service`, enables and verifies active systemd service, and enters the interactive TUI:
```bash
sudo ./mieru-admin setup
```

Non-interactive setup (for automated scripts):
```bash
sudo ./mieru-admin setup --no-tui
```

Flags:
- `--no-tui`: Do not launch TUI after successful setup.
- `--token-file <path>`: Custom token file path (default `/etc/mieru-admin/token`).
- `--config-file <path>`: Custom server configuration path (default `/etc/mita/server_config.json`).

### 2. `serve` (Daemon Mode)
Runs the management HTTP API daemon bound strictly to loopback (`127.0.0.1:9081`):
```bash
mieru-admin serve --listen 127.0.0.1:9081 --token-file /etc/mieru-admin/token
```

### 3. `tui` (Interactive Terminal Interface)
Launches the BubbleTea terminal UI:
```bash
sudo mieru-admin tui
```
Reads token automatically from `/etc/mieru-admin/token` (or `MIERU_ADMIN_TOKEN` environment variable).

### 4. `version`
Displays version, git commit, build timestamp, and runtime architecture:
```bash
mieru-admin version
```

---

## HTTP API Specification

The typed OpenAPI 3.0 specification is available at `server/api/openapi.yaml`.

Endpoints overview (`http://127.0.0.1:9081/v1`):
- `GET /v1/status` — OS, systemd, and mita proxy status (distinguishes systemd active vs mita proxy running).
- `POST /v1/installation` — Installs official pinned mita deb (`3.38.0`, SHA256 verified).
- `GET /v1/config` — Returns active sanitized configuration (passwords and hashes stripped) with revision hash.
- `POST /v1/config/preview` — Returns unified diff and upstream schema validation without applying changes.
- `PUT /v1/config` — Applies full configuration with optimistic revision locking and rollback.
- `GET /v1/users` — Lists registered users (sanitized).
- `POST /v1/users` — Adds user with upstream password hashing.
- `PATCH /v1/users/{name}` — Resets user password.
- `DELETE /v1/users/{name}` — Deletes user with explicit confirmation.
- `POST /v1/service` — Controls service (`start`, `stop`, `restart`).
- `GET /v1/logs` — Retrieves sanitized journalctl logs (credentials and tokens redacted).
- `POST /v1/profiles/export` — Validates password against server hash and exports `ClientProfile` matching `configs/profile.schema.json`.

---

## Remote Access via SSH Tunnel (Future Desktop Integration)

Public binding and CORS are deliberately disabled for security. Future desktop applications connect securely via SSH port forwarding:

```bash
ssh -L 9081:127.0.0.1:9081 root@<server-ip>
```

The desktop app reads the token via SFTP/SSH from `/etc/mieru-admin/token` and communicates with `http://127.0.0.1:9081/v1` using standard Bearer authorization.
