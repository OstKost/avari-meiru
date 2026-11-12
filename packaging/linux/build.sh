#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PKG="$ROOT/packaging/linux"
OUT="$PKG/dist"
STAGE="$PKG/.cache/context"
VERSION="${VERSION:-0.1.0}"

BUILD_ONLY="${BUILD_ONLY:-0}"
PACK_ONLY="${PACK_ONLY:-0}"
VERIFY_ONLY="${VERIFY_ONLY:-0}"

for arg in "$@"; do
  case "$arg" in
    --build-only) BUILD_ONLY=1 ;;
    --pack-only) PACK_ONLY=1 ;;
    --verify-only) VERIFY_ONLY=1 ;;
    --help|-h)
      echo "Usage: $0 [--build-only | --pack-only | --verify-only]"
      exit 0
      ;;
  esac
done

mkdir -p "$OUT" "$PKG/.cache"

if [ "$VERIFY_ONLY" = 1 ]; then
  bash "$PKG/verify-packages.sh" "$VERSION"
  exit 0
fi

if [ "$PACK_ONLY" = 1 ]; then
  if [ ! -d "$OUT/build/out" ]; then
    echo "Error: pack-only requires existing build output at $OUT/build/out" >&2
    exit 2
  fi
  echo "=== Running pack-only mode for version $VERSION ==="
else
  echo "=== Running Linux release build for version $VERSION ==="

  # UI dist handling
  UI_BUILD_MODE="freshly built with frozen-lockfile"
  if [ "$BUILD_ONLY" = 1 ] && [ -d "$ROOT/desktop/ui/dist" ]; then
    UI_BUILD_MODE="reused existing desktop/ui/dist"
    echo "build-only mode: reusing existing desktop/ui/dist"
  else
    echo "Building UI frontend (frozen-lockfile)..."
    pnpm --dir "$ROOT/desktop/ui" install --frozen-lockfile
    pnpm --dir "$ROOT/desktop/ui" build
  fi

  rm -rf "$STAGE"
  mkdir -p "$STAGE/desktop/src-tauri" "$STAGE/desktop/ui" "$STAGE/core/dist" "$STAGE/packaging/linux/arch"

  # Stage allowlisted inputs
  python3 - "$ROOT" "$STAGE" <<'PYSTAGE'
import pathlib, shutil, sys
root, stage = map(pathlib.Path, sys.argv[1:])

# desktop/src-tauri
base = root / 'desktop/src-tauri'; target = stage / 'desktop/src-tauri'
for name in ('Cargo.toml', 'Cargo.lock', 'build.rs', 'tauri.conf.json'):
    p = base / name
    if p.exists(): shutil.copy2(p, target / name)
for name in ('src', 'crates', 'permissions', 'capabilities', 'icons', 'linux'):
    p = base / name
    if p.exists(): shutil.copytree(p, target / name, ignore=shutil.ignore_patterns('target', '.git', 'node_modules', '.cache'))

# core/dist
for name in ('sing-box-x86_64-unknown-linux-gnu', 'LICENSE', 'REVISION', 'linux-REVISION'):
    p = root / 'core/dist' / name
    if p.exists(): shutil.copy2(p, stage / 'core/dist' / name)
p = root / 'core/dist/licenses'
if p.exists(): shutil.copytree(p, stage / 'core/dist/licenses')

# packaging/linux
for name in ('Dockerfile', 'package-deb.sh', 'package-arch.sh', 'verify-packages.sh'):
    p = root / 'packaging/linux' / name
    if p.exists(): shutil.copy2(p, stage / 'packaging/linux' / name)
p = root / 'packaging/linux/arch/PKGBUILD'
if p.exists(): shutil.copy2(p, stage / 'packaging/linux/arch/PKGBUILD')
PYSTAGE

  mkdir -p "$STAGE/desktop/ui"
  cp -a "$ROOT/desktop/ui/dist" "$STAGE/desktop/ui/dist"

  mkdir -p "$OUT/build"

  # Generate SOURCE-MANIFEST.txt strictly from staged context
  python3 - "$STAGE" "$OUT/build/SOURCE-MANIFEST.txt" "$UI_BUILD_MODE" "$VERSION" <<'PYMAN'
import hashlib, pathlib, sys
stage = pathlib.Path(sys.argv[1])
out = pathlib.Path(sys.argv[2])
ui_mode = sys.argv[3]
version = sys.argv[4]

items = [
    'desktop/src-tauri/Cargo.toml',
    'desktop/src-tauri/Cargo.lock',
    'desktop/src-tauri/build.rs',
    'desktop/src-tauri/tauri.conf.json',
    'desktop/src-tauri/src',
    'desktop/src-tauri/crates',
    'desktop/src-tauri/permissions',
    'desktop/src-tauri/capabilities',
    'desktop/src-tauri/icons',
    'desktop/src-tauri/linux',
    'desktop/ui/dist',
    'core/dist/sing-box-x86_64-unknown-linux-gnu',
    'core/dist/LICENSE',
    'core/dist/linux-REVISION'
]

def digest(path):
    h = hashlib.sha256()
    files = [path] if path.is_file() else sorted(p for p in path.rglob('*') if p.is_file())
    for f in files:
        rel = f.relative_to(path.parent if path.is_file() else path)
        h.update(str(rel).encode() + b'\0')
        h.update(f.read_bytes())
    return h.hexdigest()

with out.open('w') as f:
    f.write(f'# Mieru source input manifest version {version}\n')
    f.write(f'# UI build mode: {ui_mode}\n')
    for item in items:
        p = stage / item
        if p.exists():
            f.write(f'{digest(p)}  {item}\n')
PYMAN

  echo "Staged context size: $(du -sh "$STAGE" | awk '{print $1}')"

  # Docker build Linux binaries
  docker build --platform linux/amd64 -f "$STAGE/packaging/linux/Dockerfile" \
    --build-arg VERSION="$VERSION" \
    --output "type=local,dest=$OUT/build" "$STAGE"

  # Record ELF hashes
  sha256sum "$OUT/build/out/usr/bin/mieru" \
            "$OUT/build/out/usr/lib/mieru/mieru-helper" \
            "$OUT/build/out/usr/lib/mieru/sing-box" > "$OUT/build/ELF-SHA256SUMS.txt"

  # Record toolchain & base image digests
  {
    echo "# Mieru Linux build toolchain and container digests"
    echo "Date: $(date -u +'%Y-%m-%dT%H:%M:%SZ')"
    echo "Rust: 1.98.1"
    echo "Revision: $(cat "$ROOT/core/dist/linux-REVISION" 2>/dev/null || cat "$ROOT/core/dist/REVISION" 2>/dev/null || echo 'unknown')"
    for img in "ubuntu:24.04" "debian:13" "archlinux:latest"; do
      digest=$(docker inspect --format='{{range .RepoDigests}}{{.}}{{"\n"}}{{end}}' "$img" 2>/dev/null | head -n1 || echo "not-pulled")
      echo "Image $img: $digest"
    done
  } > "$OUT/build/TOOLCHAIN-DIGESTS.txt"

  if [ "$BUILD_ONLY" = 1 ]; then
    echo "Build-only complete: artifacts in $OUT/build"
    exit 0
  fi
fi

# Package deb (Ubuntu 24.04 and Debian 13)
for spec in 'ubuntu:24.04 ubuntu' 'debian:13 debian'; do
  read -r image distro <<<"$spec"
  echo "Packaging for $distro ($image)..."
  docker run --rm --platform linux/amd64 \
    -v "$OUT/build:/input:ro" \
    -v "$PKG:/scripts:ro" \
    -v "$OUT:/output" \
    "$image" bash /scripts/package-deb.sh "$distro" "$VERSION" /output

  # Verification test inside container: install, query, remove
  echo "Testing install/remove in $image..."
  docker run --rm --platform linux/amd64 \
    -e PKGFILE="mieru_${VERSION}_amd64_${distro}.deb" \
    -v "$OUT:/output" \
    "$image" bash -ec 'apt-get update >/dev/null && apt-get install -y /output/$PKGFILE >/dev/null && dpkg -s mieru >/dev/null && apt-get remove -y mieru >/dev/null && ! dpkg -s mieru >/dev/null 2>&1'
done

# Package Arch Linux
echo "Packaging for Arch Linux (archlinux:latest)..."
docker run --rm --platform linux/amd64 \
  -v "$OUT/build:/input:ro" \
  -v "$PKG:/scripts:ro" \
  -v "$OUT:/output" \
  archlinux:latest bash /scripts/package-arch.sh "$VERSION" /output

# Verify all 3 packages
bash "$PKG/verify-packages.sh" "$VERSION"

# Record release SHA256SUMS
(cd "$OUT" && sha256sum \
  "mieru_${VERSION}_amd64_ubuntu.deb" \
  "mieru_${VERSION}_amd64_debian.deb" \
  "mieru-${VERSION}-1-x86_64.pkg.tar.zst" \
  "arch/PKGBUILD" \
  "arch/out.tar.gz" \
  "build/SOURCE-MANIFEST.txt" \
  "build/ELF-SHA256SUMS.txt" \
  "build/TOOLCHAIN-DIGESTS.txt" > SHA256SUMS)

echo "=== Release packages and verification completed successfully ==="
cat "$OUT/SHA256SUMS"
