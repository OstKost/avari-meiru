#!/bin/bash
set -euo pipefail
dist="$1" version="$2" output="$3"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/root/DEBIAN"
cp -a /input/out/. "$tmp/root/"
case "$dist" in
 ubuntu) deps='libc6, libgtk-3-0t64, libwebkit2gtk-4.1-0, libayatana-appindicator3-1, libssl3t64, polkitd, pkexec' ;;
 debian) deps='libc6, libgtk-3-0, libwebkit2gtk-4.1-0, libayatana-appindicator3-1, libssl3, polkitd, pkexec' ;;
 *) echo "unsupported distro: $dist" >&2; exit 2;;
esac
cat > "$tmp/root/DEBIAN/control" <<CONTROL
Package: mieru
Version: $version
Section: net
Priority: optional
Architecture: amd64
Maintainer: Mieru Project
Depends: $deps
Description: Mieru desktop VPN client
CONTROL
chmod 755 "$tmp/root/usr/bin/mieru" "$tmp/root/usr/lib/mieru/mieru-helper" "$tmp/root/usr/lib/mieru/sing-box"
chmod 644 "$tmp/root/usr/share/polkit-1/actions/app.mieru.policy"
chmod 644 "$tmp/root/usr/share/applications/mieru.desktop"
chmod 644 "$tmp/root/usr/share/icons/hicolor/128x128/apps/mieru.png"
chmod 644 "$tmp/root/usr/share/doc/mieru/"*
chmod 755 "$tmp/root/DEBIAN"
chmod 644 "$tmp/root/DEBIAN/control"
dpkg-deb --root-owner-group --build "$tmp/root" "$output/mieru_${version}_amd64_${dist}.deb"
dpkg-deb --contents "$output/mieru_${version}_amd64_${dist}.deb"
