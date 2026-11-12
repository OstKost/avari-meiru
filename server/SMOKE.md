# Factual Smoke Report: Mieru Admin (mieru-admin)

Date: 2026-11-12
Host: macOS darwin/arm64
Target Architecture: Linux x86_64 (amd64)
Pinned Components:
- Mieru mita: `3.38.0` (deb sha256: `5170555aace38f76862541587b632d61848bba12f2e845b057977c9f9257e59a`)
- BubbleTea: `v1.3.10`
- sing-box (mbox): `1.14.3` (revision `1c5b39718ebcd05e6adb31f95246fbf056be9bd7`)

---

## 1. Unit & Regression Tests Execution

Command executed: `go test -count=1 -v ./...` in `server/`
Exit Code: `0`

Summary of tests:
- `mieru-admin/internal/domain`:
  - `TestHashAndVerifyPassword`: PASS
  - `TestConfigPreservesAllSchemaFields`: PASS
  - `TestConflictDetection`: PASS
  - `TestProfileExport`: PASS
  - `TestSanitizedCopy`: PASS
  - `TestRollbackAndRestore`: PASS
  - `TestConfigIntegrity_StaleJSONWithModifiedNative`: PASS
  - `TestConfigIntegrity_CorruptedPBNoWrite`: PASS
  - `TestConfigIntegrity_RoundtripAllSchemaFields`: PASS
  - `TestConfigIntegrity_FailureNativeWriteRollback`: PASS
- `mieru-admin/internal/service`:
  - `TestAdminService_GetStatusAndConfig`: PASS
  - `TestAdminService_UserLifecycle`: PASS
  - `TestAdminService_ApplyRollbackOnFailure`: PASS
  - `TestAdminService_PreviewConfig`: PASS
  - `TestAdminService_ServiceControl`: PASS
- `mieru-admin/internal/transport/http`:
  - `TestAuthMiddleware` (Bearer token check, unauthorized 401): PASS
  - `TestHTTP_ConfigAndConflict` (GET/PUT config, preview, conflict 409): PASS
  - `TestHTTP_UserLifecycleAndExport` (POST/PATCH/DELETE user, export with password verify): PASS
  - `TestHTTP_ServiceControlAndLogs` (start/stop/restart, sanitized logs): PASS
- `mieru-admin/internal/tui`:
  - `TestTUI_ResponsiveLayout80x24`: PASS
  - `TestTUI_PasswordMasking` (EchoPassword mode): PASS
  - `TestTUI_NavigationAndEscCtrlC`: PASS

---

## 2. Release Artifacts & Checksums

Build Command: `GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build -trimpath -ldflags="-s -w -buildid=" -o dist/mieru-admin ./cmd/mieru-admin`
Binary Inspection: `ELF 64-bit LSB executable, x86-64, statically linked, stripped`

Checksums (`server/dist/SHA256SUMS`):
```text
5fe31a307af110210fd3496116c556b2e2fc26ad4600753347bf20b66f69074f  mieru-admin
19dcf5cfd7a0a2862611ec99faebd5084f091405152e68261b8686896c1b72a6  mieru-admin.service
76a46f9cb28304e9314847209d64b436b1aa13f3461ec80cadbc165821042082  README.md
5170555aace38f76862541587b632d61848bba12f2e845b057977c9f9257e59a  mita_3.38.0_amd64.deb
94d76e09b6cf1f54abd92466bbff5bf44efbe75245935ed16cf6dfa8fca18588  mieru-admin-linux-amd64.tar.gz
```

---

## 3. Config Integrity Gaps Addressed (Pre-VM Handoff)

1. **Authoritative Native `.pb` Priority & Corrupt Protection**:
   - `ConfigManager.loadConfigLocked()` treats `/etc/mita/server.conf.pb` as the primary authoritative configuration file.
   - If `.pb` is corrupted or empty, it returns `ErrUnsupportedFormat` and never falls through to default.
   - If both native `.pb` and `server_config.json` exist, native `.pb` takes precedence; if native was modified externally, attempts to save stale JSON revisions are rejected with `ErrConflict` (`TestConfigIntegrity_StaleJSONWithModifiedNative`).
   - Default initial configuration is only returned on an explicit fresh install when neither native `.pb` nor JSON exists.

2. **Official Protobuf JSON Converter & Roundtrip Preservation**:
   - `RawConfig.ToProto()` delegates directly to `common.UnmarshalJSON` (upstream official protojson unmarshaler).
   - Preserves all schema fields without loss: `dns`, `trafficPattern`, `advancedSettings`, `quotas`, `allowPrivateIP`, `allowLoopbackIP`, `portBindings`, `mtu`, `loggingLevel` (`TestConfigIntegrity_RoundtripAllSchemaFields`).
   - Fails closed on unsupported unknown formats (`DiscardUnknown: false`).

3. **Strict Error Handling & Dual-Snapshot Rollback**:
   - `SaveConfig` checks `WriteNativePb` return code; on failure, restores both JSON and native `.pb` with original owner/mode and reports failure. It never claims rolled back if restore fails (`TestConfigIntegrity_FailureNativeWriteRollback`).
   - `ApplyConfig` verifies `GetMitaStatus()` errors (`proxyStatusKnown`). Indeterminate status is not assumed to be stopped, preventing unintended shutdown of running services. Rollback errors are propagated without false success claims.

---

## 4. Container Verification: Ubuntu 24.04 & Debian 13

### Debian 13 x86_64 Container (`debian:trixie-slim`)
- Package installation: `dpkg -i /server/dist/mita_3.38.0_amd64.deb` -> `mita 3.38.0` OK.
- Management daemon `/server/dist/mieru-admin serve` started on `127.0.0.1:9081`.
- API `/v1/status` -> HTTP 200 OK.
- API `/v1/users` -> Created user `e2e_test@example.com` (HTTP 201).
- API `/v1/profiles/export` -> HTTP 200 with valid password, HTTP 401 with invalid password.
- Mita TCP proxy bound to `127.0.0.1:28443`.
- Sing-box client connected to Mita proxy: `curl -x socks5h://127.0.0.1:1080 http://example.com` returned HTTP 200 OK.
- Sing-box client with wrong password rejected: HTTP 000.
- Result: **ALL END-TO-END CONTAINER TESTS PASSED STRICTLY!**

### Ubuntu 24.04 x86_64 Container (`ubuntu:24.04`)
- Package installation: `dpkg -i /server/dist/mita_3.38.0_amd64.deb` -> `mita 3.38.0` OK.
- Management daemon `/server/dist/mieru-admin serve` started on `127.0.0.1:9081`.
- API `/v1/status` -> HTTP 200 OK.
- API `/v1/users` -> Created user `e2e_test@example.com` (HTTP 201).
- API `/v1/profiles/export` -> HTTP 200 with valid password, HTTP 401 with invalid password.
- Mita TCP proxy bound to `127.0.0.1:28443`.
- Sing-box client connected to Mita proxy: `curl -x socks5h://127.0.0.1:1080 http://example.com` returned HTTP 200 OK.
- Sing-box client with wrong password rejected: HTTP 000.
- Result: **ALL END-TO-END CONTAINER TESTS PASSED STRICTLY!**

---

## 5. Live PTY Test Execution (`live_pty_test.py`)

Command executed: `python3 test/live_pty_test.py /tmp/mieru-admin-macos http://127.0.0.1:9081/v1 secret-token-pty-12345`
Exit Code: `0`

Output:
```text
=== Starting Live PTY Verification for /tmp/mieru-admin-macos ===
API: http://127.0.0.1:9081/v1, TokenFile: /tmp/test-pty-token
[1/4] Waiting for initial dashboard render on 80x24 PTY...
✓ Initial dashboard rendered successfully in 80x24 PTY
[2/4] Testing live PTY resize to 100x30...
✓ SIGWINCH resize handled without crashing
[3/4] Testing navigation: sending '5' (Service screen)...
✓ Navigated to Service screen
[4/4] Testing Esc return and clean 'q' exit...
✓ Verified zero secret token leakage in PTY output
✓ TUI exited cleanly with returncode 0 and terminal state restored
=== LIVE PTY TEST PASSED ===
```

---

## 6. Native systemd Acceptance Execution in Real Ubuntu 24.04 VM

- **Target:** Ubuntu 24.04.5 LTS x86_64 under genuine systemd PID 1
- **Package Installed:** `mita_3.38.0_amd64.deb` (`/usr/lib/systemd/system/mita.service`)
- **Setup Command:** `sudo /home/testguest/server_test/mieru-admin setup --no-tui`
- **Exit Code:** `0`

### Execution Output:
```text
=== Mieru Admin Setup ===
Installing binary to /usr/local/bin/mieru-admin...
Binary installed to /usr/local/bin/mieru-admin successfully
Configuring authentication token at /etc/mieru-admin/token...
Authentication token configured (permissions 0600, root owned)
Installing systemd unit at /etc/systemd/system/mieru-admin.service...
Systemd service enabled, started, and verified active
Setup completed successfully!
```

### Systemd Verification:
```text
● mieru-admin.service - Mieru Admin Management Daemon
     Loaded: loaded (/etc/systemd/system/mieru-admin.service; enabled; preset: enabled)
     Active: active (running)
   Main PID: 1354 (mieru-admin)
     CGroup: /system.slice/mieru-admin.service
             └─1354 /usr/local/bin/mieru-admin serve --token-file=/etc/mieru-admin/token --config-file=/etc/mita/server_config.json
```

### API Verification:
1. **Unauthorized Access:** `GET http://127.0.0.1:9081/v1/status` without token -> `401 Unauthorized`
2. **Status Query:** `GET http://127.0.0.1:9081/v1/status` with Bearer token:
   ```json
   {"os":{"name":"Ubuntu 24.04.5 LTS","version":"24.04","arch":"amd64","systemd":true,"public_ip":"10.0.2.15"},"mita":{"installed":true,"version":"3.38.0","expected_version":"3.38.0","systemd_active":true,"daemon_running":true,"proxy_running":false,"port":28443,"users_count":0},"mieru_admin":{"version":"1.0.0","commit":"dev","uptime_seconds":16}}
   ```
3. **Empty/Corrupted Native Configuration Protection:**
   When `/etc/mita/server.conf.pb` was 0 bytes, queries returned `ErrUnsupportedFormat`:
   `{"code":"LOAD_CONFIG_FAILED","message":"unsupported or corrupted configuration format: authoritative native pb /etc/mita/server.conf.pb is empty"}` (failing closed strictly).
4. **User Lifecycle & Authoritative Protobuf Generation:**
   - `POST /v1/users`: created `vm_test_user` -> `{"name":"vm_test_user","revision":"...","success":true}`
   - `GET /v1/users`: `{"users":[{"name":"vm_test_user"}]}`
   - Verified filesystem: `/etc/mita/server.conf.pb` (99 bytes, root:root 0600), `/etc/mita/server_config.json` (383 bytes), `/etc/mita/backups/` snapshot created.
5. **Profile Export:**
   - `POST /v1/profiles/export`:
     `{"server":"10.0.2.15","server_port":28443,"transport":"TCP","username":"vm_test_user","password":"Passw0rd12345678!"}`
6. **Service Control & Proxy Execution:**
   - `POST /v1/service` with `{"action":"restart"}` -> `{"success":true,"action":"restart","systemd_active":true,"mita_running":true}`
   - Verified listening socket via `ss -tulpn | grep 28443`:
     `tcp LISTEN 0 4096 *:28443 *:*` (mita actively listening on port 28443).

**Result: NATIVE SYSTEMD ACCEPTANCE TEST PASSED SUCCESSFULLY!**
