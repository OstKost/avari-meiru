# Linux x86_64 packages

Version 0.1.1 adds a Linux startup workaround for WebKitGTK DMABUF crashes. All three packages passed install/remove and payload verification:

- `dist/mieru_0.1.1_amd64_ubuntu.deb` — Ubuntu 24.04
- `dist/mieru_0.1.1_amd64_debian.deb` — Debian 13
- `dist/mieru-0.1.1-1-x86_64.pkg.tar.zst` — Arch Linux

All install `/usr/bin/mieru`, `/usr/lib/mieru/mieru-helper`, `/usr/lib/mieru/sing-box`, the polkit action, desktop entry, icon, and license notices. GUI runs as the desktop user; only helper elevation uses pkexec. Packages do not start a VPN at install time. Release downloads: [v0.1.1](https://github.com/OstKost/avari-meiru/releases/tag/v0.1.1). No AUR submission is performed.

## Version 0.1.1 build snapshot

Build command: `./packaging/linux/build.sh --build-only` (exit 0; current-source Linux release build, existing UI dist reused). Source input hashes are recorded in `dist/build/SOURCE-MANIFEST.txt`; ELF hashes are recorded in `dist/build/ELF-SHA256SUMS.txt`.

- GUI: `dist/build/out/usr/bin/mieru`, SHA256 `1dd943a36571dc9f61fd3b023e8baef2869be7e12a982cbdbbffde76231efd9b`
- Helper: `dist/build/out/usr/lib/mieru/mieru-helper`, SHA256 `15226e772787850e2bc52ab7467c1583c85db6cbb76707491b7789e212bd8f47`

Build input hashes match the current source (`SOURCE_MANIFEST_MATCH_OK`). The helper and pinned core bytes are unchanged. Existing version 0.1.0 package files are retained.

The first Arch attempt failed on mirror download timeouts. After adding fallback mirrors, the Arch-only retry passed (`ARCH_INSTALLED_OK`, `ARCH_REMOVED_OK`, `ldd` without missing libraries). `verify-packages.sh` passed for all three packages and the standalone Arch recipe: payload hashes match staging, ownership/modes and polkit policy checks passed. All eight entries of `dist/SHA256SUMS` verified successfully.

## DMABUF compatibility default

On Linux, `main()` sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` before Tauri starts, only when the variable is absent. This applies to menu and terminal launches on every Linux distribution. Explicit values are preserved. To opt back into DMABUF, launch `WEBKIT_DISABLE_DMABUF_RENDERER=0 mieru`. For the older package, use `WEBKIT_DISABLE_DMABUF_RENDERER=1 mieru` as a manual workaround.

Absent, `0`, `1`, empty and non-UTF-8 environment values passed subprocess checks using the actual startup block. Version 0.1.1 GUI smoke passed in Ubuntu/Xvfb: visible rendered window, profile navigation and D-BusMenu Quit with exit 0. NVIDIA/Wayland remains untested here. Details and limitations: [docs/LINUX_DMABUF_FIX.md](../../docs/LINUX_DMABUF_FIX.md).

## Runtime scope

Historical version 0.1.0 GUI runtime checks passed on Linux: connection, DNS and HTTPS 200, disconnect/reconnect cleanup, tray hide/show/quit, owner-death cleanup, and health watchdog readiness/cleanup. These checks used a local Xvfb/session and test service; they do not prove interaction with every real desktop authentication dialog or polkit agent. A real desktop authentication-dialog check remains outside this package verification. This status does not claim all Linux distributions or desktop environments have been exhaustively validated.
