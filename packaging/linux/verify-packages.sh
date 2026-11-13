#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PKG="$ROOT/packaging/linux"
OUT="$PKG/dist"
BUILD_OUT="$OUT/build/out"
VERSION="${1:-${VERSION:-0.1.1}}"

echo "=== Verifying packages in $OUT against $BUILD_OUT (version: $VERSION) ==="

if [ ! -d "$BUILD_OUT" ]; then
  echo "Error: expected build out directory $BUILD_OUT does not exist." >&2
  exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Reference hashes
GUI_REF_HASH=$(sha256sum "$BUILD_OUT/usr/bin/mieru" | awk '{print $1}')
HELPER_REF_HASH=$(sha256sum "$BUILD_OUT/usr/lib/mieru/mieru-helper" | awk '{print $1}')
SINGBOX_REF_HASH=$(sha256sum "$BUILD_OUT/usr/lib/mieru/sing-box" | awk '{print $1}')

echo "Reference hashes:"
echo "  mieru:        $GUI_REF_HASH"
echo "  mieru-helper: $HELPER_REF_HASH"
echo "  sing-box:     $SINGBOX_REF_HASH"

verify_deb() {
  local distro="$1"
  local deb="$OUT/mieru_${VERSION}_amd64_${distro}.deb"
  echo "--- Verifying $deb ---"
  test -f "$deb" || { echo "Missing $deb" >&2; return 1; }

  local dir="$TMP/$distro"
  mkdir -p "$dir"

  # Unpack deb using ar
  (cd "$dir" && ar -x "$deb")
  mkdir -p "$dir/data"
  local data_archive=""
  if [ -f "$dir/data.tar.zst" ]; then
    zstd -d "$dir/data.tar.zst" -o "$dir/data.tar"
    data_archive="$dir/data.tar"
  elif [ -f "$dir/data.tar.xz" ]; then
    data_archive="$dir/data.tar.xz"
  elif [ -f "$dir/data.tar.gz" ]; then
    data_archive="$dir/data.tar.gz"
  else
    echo "Unknown data archive format in $deb" >&2
    return 1
  fi
  tar -C "$dir/data" -xf "$data_archive"

  # 1. Payload hashes
  local gui_h helper_h singbox_h
  gui_h=$(sha256sum "$dir/data/usr/bin/mieru" | awk '{print $1}')
  helper_h=$(sha256sum "$dir/data/usr/lib/mieru/mieru-helper" | awk '{print $1}')
  singbox_h=$(sha256sum "$dir/data/usr/lib/mieru/sing-box" | awk '{print $1}')

  test "$gui_h" = "$GUI_REF_HASH" || { echo "GUI hash mismatch in $distro: $gui_h vs $GUI_REF_HASH" >&2; return 1; }
  test "$helper_h" = "$HELPER_REF_HASH" || { echo "Helper hash mismatch in $distro: $helper_h vs $HELPER_REF_HASH" >&2; return 1; }
  test "$singbox_h" = "$SINGBOX_REF_HASH" || { echo "sing-box hash mismatch in $distro: $singbox_h vs $SINGBOX_REF_HASH" >&2; return 1; }

  # 2. Policy check: no allow_gui
  local pol="$dir/data/usr/share/polkit-1/actions/app.mieru.policy"
  test -f "$pol" || { echo "Missing policy file in $distro deb" >&2; return 1; }
  if grep -qi 'allow_gui' "$pol"; then
    echo "ERROR: allow_gui found in policy of $distro deb" >&2
    return 1
  fi

  # 3. Ownership and permissions
  python3 - "$data_archive" <<'PYDEB'
import sys, tarfile
t = tarfile.open(sys.argv[1], 'r:*')
for m in t.getmembers():
    if m.name in ('.', './'): continue
    if m.uid != 0 or m.gid != 0:
        sys.exit(f"Non-root ownership in deb: {m.name} uid={m.uid} gid={m.gid}")
    if m.name in ('./usr/bin/mieru', './usr/lib/mieru/mieru-helper', './usr/lib/mieru/sing-box'):
        if oct(m.mode)[-3:] != '755':
            sys.exit(f"Invalid binary mode in deb: {m.name} {oct(m.mode)}")
    elif m.isfile():
        if oct(m.mode)[-3:] != '644':
            sys.exit(f"Invalid file mode in deb: {m.name} {oct(m.mode)}")
PYDEB

  echo "  $distro deb: verified successfully."
}

verify_arch() {
  local pkg="$OUT/mieru-${VERSION}-1-x86_64.pkg.tar.zst"
  echo "--- Verifying Arch package: $pkg ---"
  test -f "$pkg" || { echo "Missing $pkg" >&2; return 1; }

  local dir="$TMP/arch"
  mkdir -p "$dir/pkg"
  zstd -d "$pkg" -o "$dir/pkg.tar"
  tar -C "$dir/pkg" -xf "$dir/pkg.tar"

  # 1. Payload hashes
  local gui_h helper_h singbox_h
  gui_h=$(sha256sum "$dir/pkg/usr/bin/mieru" | awk '{print $1}')
  helper_h=$(sha256sum "$dir/pkg/usr/lib/mieru/mieru-helper" | awk '{print $1}')
  singbox_h=$(sha256sum "$dir/pkg/usr/lib/mieru/sing-box" | awk '{print $1}')

  test "$gui_h" = "$GUI_REF_HASH" || { echo "GUI hash mismatch in Arch: $gui_h vs $GUI_REF_HASH" >&2; return 1; }
  test "$helper_h" = "$HELPER_REF_HASH" || { echo "Helper hash mismatch in Arch: $helper_h vs $HELPER_REF_HASH" >&2; return 1; }
  test "$singbox_h" = "$SINGBOX_REF_HASH" || { echo "sing-box hash mismatch in Arch: $singbox_h vs $SINGBOX_REF_HASH" >&2; return 1; }

  # 2. Policy check: no allow_gui
  local pol="$dir/pkg/usr/share/polkit-1/actions/app.mieru.policy"
  test -f "$pol" || { echo "Missing policy file in Arch pkg" >&2; return 1; }
  if grep -qi 'allow_gui' "$pol"; then
    echo "ERROR: allow_gui found in policy of Arch pkg" >&2
    return 1
  fi

  # 3. Ownership and permissions
  python3 - "$dir/pkg.tar" <<'PYARCH'
import sys, tarfile
t = tarfile.open(sys.argv[1])
for m in t.getmembers():
    if m.name.startswith('.PKGINFO') or m.name.startswith('.MTREE') or m.name.startswith('.BUILDINFO'): continue
    if m.name in ('.', './'): continue
    if m.uid != 0 or m.gid != 0:
        sys.exit(f"Non-root ownership in Arch pkg: {m.name} uid={m.uid} gid={m.gid}")
    if m.name in ('usr/bin/mieru', 'usr/lib/mieru/mieru-helper', 'usr/lib/mieru/sing-box'):
        if oct(m.mode)[-3:] != '755':
            sys.exit(f"Invalid binary mode in Arch pkg: {m.name} {oct(m.mode)}")
    elif m.isfile():
        if oct(m.mode)[-3:] != '644':
            sys.exit(f"Invalid file mode in Arch pkg: {m.name} {oct(m.mode)}")
PYARCH

  # 4. Check standalone PKGBUILD & source archive
  local pkgbuild="$OUT/arch/PKGBUILD"
  local source_archive="$OUT/arch/out.tar.gz"
  test -f "$pkgbuild" || { echo "Missing standalone PKGBUILD at $pkgbuild" >&2; return 1; }
  test -f "$source_archive" || { echo "Missing standalone source archive at $source_archive" >&2; return 1; }

  if grep -q 'REPLACE_HASH' "$pkgbuild"; then
    echo "ERROR: PKGBUILD contains placeholder REPLACE_HASH" >&2
    return 1
  fi

  local arch_source_hash expected_hash
  arch_source_hash=$(sha256sum "$source_archive" | awk '{print $1}')
  expected_hash=$(grep "sha256sums=" "$pkgbuild" | sed -E "s/.*'([a-f0-9]+)'.*/\1/")
  test "$arch_source_hash" = "$expected_hash" || {
    echo "ERROR: PKGBUILD sha256sum mismatch: $expected_hash vs actual $arch_source_hash" >&2
    return 1
  }

  echo "  Arch package and standalone recipe: verified successfully."
}

verify_deb "ubuntu"
verify_deb "debian"
verify_arch

echo "=== All 3 packages and standalone Arch recipe verified OK ==="
