# Linux x86_64 packages

Final package artifacts, built from the currently accepted GUI/helper snapshot:

- `dist/mieru_0.1.0_amd64_ubuntu.deb` — Ubuntu 24.04
- `dist/mieru_0.1.0_amd64_debian.deb` — Debian 13
- `dist/mieru-0.1.0-1-x86_64.pkg.tar.zst` — Arch Linux

All install `/usr/bin/mieru`, `/usr/lib/mieru/mieru-helper`, `/usr/lib/mieru/sing-box`, the polkit action, desktop entry, icon, and license notices. GUI runs as the desktop user; only helper elevation uses pkexec. Packages do not start a VPN at install time. No publication or AUR submission is performed.

## Accepted build snapshot

Build command: `./packaging/linux/build.sh --build-only` (exit 0; current-source Linux release build, existing UI dist reused). Source input hashes are recorded in `dist/build/SOURCE-MANIFEST.txt`; ELF hashes are recorded in `dist/build/ELF-SHA256SUMS.txt`.

- GUI: `dist/build/out/usr/bin/mieru`, SHA256 `1f425262582d3eba9f7aa91b38f8f52276118e68d8de5f4e2eb273840159cef7`
- Helper: `dist/build/out/usr/lib/mieru/mieru-helper`, SHA256 `15226e772787850e2bc52ab7467c1583c85db6cbb76707491b7789e212bd8f47`

Packages were recreated from that existing `dist/build/out` with `package-deb.sh ubuntu 0.1.0` and `package-deb.sh debian 0.1.0` in Ubuntu 24.04 and Debian 13 containers, plus `package-arch.sh 0.1.0` in a disposable Arch container. Extracted content checks passed for all three: GUI/helper hashes match exactly, policy contains no `allow_gui`, and binaries have root ownership and mode 755. Arch produced the main `mieru-0.1.0-1-x86_64.pkg.tar.zst` (no debug package); its recipe reported `ARCH_INSTALLED_OK`, `ARCH_REMOVED_OK`, and `ldd` had no `not found`. The post-removal `pacman -Q mieru` nonzero result is expected and handled by the recipe. Earlier accepted Ubuntu, Debian and Arch install/remove checks remain applicable because package formats and dependencies did not change.

## Runtime scope

The coordinator's GUI runtime checks passed on Linux: connection, DNS and HTTPS 200, disconnect/reconnect cleanup, tray hide/show/quit, owner-death cleanup, and health watchdog readiness/cleanup. These checks used a local Xvfb/session and test service; they do not prove interaction with every real desktop authentication dialog or polkit agent. A real desktop authentication-dialog check remains outside this package verification. This status does not claim all Linux distributions or desktop environments have been exhaustively validated.
