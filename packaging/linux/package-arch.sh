#!/bin/bash
set -euxo pipefail
version="$1" output="$2"
# This runs only inside the disposable Arch container; host pacman config is untouched.
cat > /etc/pacman.d/mirrorlist <<'MIRRORS'
Server = https://mirror.osbeck.com/archlinux/$repo/os/$arch
Server = https://mirrors.kernel.org/archlinux/$repo/os/$arch
Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch
MIRRORS
sed -i "s/^#ParallelDownloads = 5/ParallelDownloads = 10/" /etc/pacman.conf
pacman --disable-sandbox-syscalls -Sy --noconfirm --needed base-devel zstd gtk3 webkit2gtk-4.1 libayatana-appindicator openssl polkit
useradd -m builder
install -d /etc/pacman.d

install -d -o builder -g builder /home/builder/src
cp -a /input/out /home/builder/src/out
tar -C /home/builder/src -czf /home/builder/out.tar.gz out
sha256=$(sha256sum /home/builder/out.tar.gz | awk '{print $1}')
cp /scripts/arch/PKGBUILD /home/builder/PKGBUILD
sed -i "s/^pkgver=.*/pkgver=$version/; s/REPLACE_HASH/$sha256/" /home/builder/PKGBUILD
chown -R builder:builder /home/builder
su builder -c 'cd /home/builder && makepkg --cleanbuild --noconfirm'
pkg="/home/builder/mieru-${version}-1-x86_64.pkg.tar.zst"
test -f "$pkg"
cp "$pkg" "$output/"
mkdir -p "$output/arch"
cp /home/builder/PKGBUILD "$output/arch/PKGBUILD"
cp /home/builder/out.tar.gz "$output/arch/out.tar.gz"
pacman --disable-sandbox-syscalls -U --noconfirm "$pkg"
pacman -Q mieru
echo ARCH_INSTALLED_OK
ldd /usr/bin/mieru | tee /tmp/mieru-ldd
! grep -q 'not found' /tmp/mieru-ldd
test -x /usr/lib/mieru/mieru-helper
test -f /usr/share/polkit-1/actions/app.mieru.policy
pacman -Rns --noconfirm mieru
if pacman -Q mieru; then echo "package unexpectedly remains installed" >&2; exit 1; else echo ARCH_REMOVED_OK; fi
