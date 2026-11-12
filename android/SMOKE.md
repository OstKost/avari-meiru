# Android Live Runtime Smoke Acceptance Report

Date: 2026-11-11
Host: macOS arm64
Target: Android 8+ (API 26+) Kotlin / Jetpack Compose Mieru Client
Device: Android Emulator `Avari_API_36` (API 36, Android 16 / Baklava, `arm64-v8a`)
Verification Time Window: 23:21 – 23:24 MSK (до отключения VPS в 23:25 МСК)
Artifact APK: `android/app/build/outputs/apk/debug/app-debug.apk`

---

## 1. Build Verification
- Command:
  ```sh
  # AAR reproduction step
  cp ../core/dist/libbox.aar app/libs/libbox.aar

  # Gradle requires JDK 17+ (e.g. JDK 17 or JDK 21)
  export JAVA_HOME="${JAVA_HOME:-$(/usr/libexec/java_home -v 17 2>/dev/null || /usr/libexec/java_home -v 21)}"
  export PATH="$JAVA_HOME/bin:$PATH"
  cd android && ./gradlew assembleDebug --no-daemon
  ```
- Result: Exit code 0 (`BUILD SUCCESSFUL in 10s`, 36 tasks, APK created: `android/app/build/outputs/apk/debug/app-debug.apk`).
- Packaging: Integrates official `libbox.aar` (`classes.jar`, package `io.nekohasekai.libbox`, native libraries `arm64-v8a` & `x86_64`).

---

## 2. Live Runtime Scenarios & Evidence

### Step 1: Baseline Network Check (Disconnected)
- Command:
  ```sh
  (printf "GET / HTTP/1.0\r\nHost: ifconfig.me\r\n\r\n"; sleep 2) | nc ifconfig.me 80
  ```
- Output:
  ```http
  HTTP/1.0 200 OK
  109.107.176.224
  ```
- Interface state: No `tun0` interface exists (`Device "tun0" does not exist`).

### Step 2: Test Profile Injection (Zero Secret Leakage)
- Command: Streamed private profile into app private storage via `adb exec-in` into `run-as org.enfein.mieru`:
  No secrets in `argv`, command history, or logs. Plaintext file immediately consumed and deleted. Credentials stored in `EncryptedSharedPreferences` / Android Keystore.
- UI display: Passwords masked (`••••••••••••••••••••••••••••••••`). `SanitizedLogger` masks registered credentials and passwords of length >= 1.

### Step 3: Connection & Tunnel Establishment
- Action: Connect triggered via UI and `MieruVpnService`.
- System evidence in logcat:
  ```text
  Vpn: Switched from [Legacy VPN] to org.enfein.mieru
  ConnectivityService: registerNetworkAgent NetworkAgentInfo{... InterfaceName: tun0 LinkAddresses: [172.19.0.1/30, fdfe:dcba:9876::1/126] DnsAddresses: [/172.19.0.2, /fdfe:dcba:9876::2] Routes: [0.0.0.0/0 -> 0.0.0.0 tun0, ::/0 -> :: tun0]}
  MieruSanitized: Set underlying networks: 2
  MieruSanitized: Tunnel verified successfully. Connected!
  ```
- UI state:
  - `VPN Status: Connected`
  - Message: `Connected to 31.58.213.192:28443`
  - Action button: `Disconnect`

### Step 4: Network Routing & External IP Verification
- External IP over HTTP:
  ```sh
  (printf "GET / HTTP/1.0\r\nHost: ifconfig.me\r\n\r\n"; sleep 2) | nc ifconfig.me 80
  ```
  Output:
  ```http
  HTTP/1.0 200 OK
  31.58.213.192
  ```
- External IP over HTTPS via Android runtime:
  - Executed TLS request to `https://api.ipify.org` using `HttpsURLConnection` via `app_process`:
  - Output:
    ```text
    HTTPS IP: 31.58.213.192
    ```
- DNS Hijack verification:
  - `ping -c 2 google.com` resolved to `172.217.23.238`, packets routed and answered with 0% loss (avg 0.55ms).
  - Background watchdog probe to `http://connectivitycheck.gstatic.com/generate_204` continuously returned `HTTP 204`.

### Step 5: Disconnect & Baseline Restoration
- Action: Disconnect tapped in UI.
- Logcat evidence:
  ```text
  MieruSanitized: Disconnecting VPN…
  MieruSanitized: Libbox stopped cleanly
  MieruSanitized: VPN disconnected cleanly
  ```
- External IP check:
  ```sh
  (printf "GET / HTTP/1.0\r\nHost: ifconfig.me\r\n\r\n"; sleep 2) | nc ifconfig.me 80
  ```
  Output:
  ```http
  HTTP/1.0 200 OK
  109.107.176.224
  ```
- Interface check:
  `adb shell ip addr show dev tun0` -> `Device "tun0" does not exist.` (Zero lingering interfaces).

### Step 6: Reconnection & Activity Lifecycle
- Reconnection: Tapped Connect again at 23:23 MSK, verified status `Connected to 31.58.213.192:28443`.
- Activity Close: Sent `KEYCODE_BACK` closing `MainActivity`.
- Verified VPN persistence: `ifconfig.me` returned `31.58.213.192` while Activity was destroyed.
- Reopen & Final Stop: Reopened `MainActivity` at 23:24 MSK, tapped `Disconnect`. Verified clean stop and immediate revert to `109.107.176.224`.
- Final Post-Test State:
  - Service state: `DISCONNECTED`
  - Active tunnel interface: None (`Device "tun0" does not exist`)
  - External IP: Baseline (`109.107.176.224`)
  - No leftover plaintext credential files or background processes.

---

## 3. Gap Checklist Status
1. **onDestroy cleanup & race elimination**: Clean synchronous release under `commandMutex.withLock` and `runBlocking(Dispatchers.IO)`. Idempotent disconnect.
2. **Independent LibboxBridge.stop()**: Separate independent `runCatching` blocks for `closeService()`, `commandServer.close()`, and `pfd.close()`. Nulls references cleanly.
3. **protect(fd) validation**: Throws `IllegalStateException` on `protect(fd) == false`.
4. **Interface Monitor & getInterfaces**: Implements real `InterfaceUpdateListener` notifications, filters out `tun*` interfaces, populates flags, CIDR addresses, and interface types per upstream contract.
5. **Secret Sanitization & Failure Detection**: Secrets of length >= 1 masked in UI, logs, and error strings. Background probe watchdog detects silent connection drop and transitions out of `CONNECTED`.
6. **DNS Hijack & IPv6 containment**: Config uses `dns_mode: "hijack"`, `route.rules: [{port: 53, action: "hijack-dns"}]`, and routes `::/0` through `tun0`.
