#!/bin/bash
# Сборка libbox.aar из закреплённой ревизии enfein/mbox.
# Скрипт не вызывает sing-box run, не открывает TUN и не меняет маршруты macOS.
# Глобальные JAVA_HOME, ANDROID_HOME и настройки SDK не записывает.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"

MBOX_REPO="https://github.com/enfein/mbox.git"
MBOX_BRANCH="release-1.14.3"
MBOX_COMMIT="1c5b39718ebcd05e6adb31f95246fbf056be9bd7"
MBOX_VERSION="1.14.3"
MBOX_URL="https://github.com/enfein/mbox/commit/${MBOX_COMMIT}"
MIERU_MODULE="github.com/enfein/mieru/v3"
MIERU_VERSION="v3.38.0"
BUILD_TAGS="with_gvisor,badlinkname,tfogo_checklinkname0"
MIN_GO="1.25.5"
ANDROID_API="26"
GOMOBILE_VERSION="v0.1.13"
NDK_REVISION="28.2.13676358"
DEFAULT_SDK="/Volumes/KingstonM2/Android/sdk"
JDK_SEMVER="17.0.21+9"
JDK_RELEASE="jdk-17.0.21+9"
JAVAPKG="io.nekohasekai"
LIBNAME="box"
JAVA_PACKAGE="${JAVAPKG}.libbox"

DIST="${ROOT}/dist"
SRC="${MBOX_SRC:-${ROOT}/.cache/mbox}"
CACHE="${ROOT}/.cache"
AAR="${DIST}/libbox.aar"
GOBIN="${CACHE}/tools/bin"
GOPATH_LOCAL="${CACHE}/gopath"
GOCACHE_LOCAL="${CACHE}/go-build-android"

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  cat <<EOF
usage: ./build-android.sh

Собирает ${AAR} из commit ${MBOX_COMMIT}.
ABI: arm64-v8a и x86_64. minSdk ${ANDROID_API}.
Теги: ${BUILD_TAGS}.
Java-пакет: ${JAVA_PACKAGE}.

JDK: Eclipse Temurin ${JDK_SEMVER} в ${CACHE}/jdk, если в окружении
ещё нет OpenJDK 17. Java 21 не используется: gomobile этой ревизии
требует строку «openjdk 17».
SDK по умолчанию: ${DEFAULT_SDK}. NDK ${NDK_REVISION} должен уже стоять.
Скрипт не вызывает sdkmanager и не меняет глобальные настройки.

Переменные:
  MBOX_SRC            чистый клон с HEAD=${MBOX_COMMIT}
  ANDROID_SDK_ROOT    или ANDROID_HOME, иначе ${DEFAULT_SDK}
  ANDROID_NDK_HOME    каталог NDK; ревизия в source.properties обязана быть ${NDK_REVISION}
  JAVA_HOME           используется только если это OpenJDK 17
EOF
}

need() {
  local cmd="$1"
  local hint="$2"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "error: не найден инструмент «${cmd}». ${hint}" >&2
    exit 127
  fi
}

go_version_at_least() {
  local actual="$1"
  local minimum="$2"
  local IFS=.
  local -a a=() b=()
  local i na nb n
  # shellcheck disable=SC2206
  a=(${actual})
  # shellcheck disable=SC2206
  b=(${minimum})
  n=${#a[@]}
  if ((${#b[@]} > n)); then
    n=${#b[@]}
  fi
  for ((i = 0; i < n; i++)); do
    na=${a[i]:-0}
    nb=${b[i]:-0}
    na=${na%%[^0-9]*}
    nb=${nb%%[^0-9]*}
    na=${na:-0}
    nb=${nb:-0}
    if ((10#$na > 10#$nb)); then
      return 0
    fi
    if ((10#$na < 10#$nb)); then
      return 1
    fi
  done
  return 0
}

check_go() {
  local raw ver
  raw="$(go env GOVERSION 2>/dev/null || true)"
  ver="${raw#go}"
  if [[ -z "$ver" || "$ver" == "$raw" ]]; then
    die "не удалось прочитать версию Go. Нужен Go ${MIN_GO} или новее. GOTOOLCHAIN=local, toolchain сам не скачивается."
  fi
  ver="${ver%%-*}"
  if ! go_version_at_least "$ver" "$MIN_GO"; then
    die "установлен Go ${ver}, а go.mod требует ${MIN_GO} или новее."
  fi
  echo "go: ${ver}"
}

ensure_source() {
  local head dirty
  if [[ -d "$SRC" ]]; then
    if [[ ! -e "$SRC/.git" ]]; then
      die "каталог исходников ${SRC} уже есть, но это не git-клон."
    fi
    if ! head="$(git -C "$SRC" rev-parse HEAD 2>/dev/null)"; then
      die "не удалось прочитать HEAD в ${SRC}."
    fi
    if [[ "$head" != "$MBOX_COMMIT" ]]; then
      die "в ${SRC} ревизия ${head}, нужна ${MBOX_COMMIT} (${MBOX_URL}). Скрипт чужой клон не переключает."
    fi
    dirty="$(git -C "$SRC" status --porcelain)"
    if [[ -n "$dirty" ]]; then
      die "клон ${SRC} содержит незакоммиченные изменения. Сборка должна совпадать с ${MBOX_COMMIT}."
    fi
    echo "source: ${SRC} @ ${head}"
    return
  fi

  if [[ -n "${MBOX_SRC:-}" ]]; then
    die "MBOX_SRC=${SRC} не существует."
  fi

  mkdir -p "$(dirname "$SRC")"
  echo "clone: ${MBOX_REPO} branch ${MBOX_BRANCH} -> ${SRC}"
  if ! GIT_TERMINAL_PROMPT=0 git clone --depth 1 --branch "$MBOX_BRANCH" "$MBOX_REPO" "$SRC"; then
    die "не удалось клонировать ${MBOX_REPO}."
  fi
  head="$(git -C "$SRC" rev-parse HEAD)"
  if [[ "$head" != "$MBOX_COMMIT" ]]; then
    die "ветка ${MBOX_BRANCH} указывает на ${head}, а не на ${MBOX_COMMIT}. Ревизия не сдвигается."
  fi
  echo "source: ${SRC} @ ${head}"
}

install_file() {
  local src="$1"
  local dest="$2"
  mkdir -p "$(dirname "$dest")"
  if [[ -e "$dest" ]]; then
    chmod u+w "$dest" || true
  fi
  cp -f "$src" "$dest"
  chmod 644 "$dest"
}

java_major_text() {
  local home="$1"
  if [[ ! -x "${home}/bin/java" ]]; then
    return 1
  fi
  "${home}/bin/java" --version 2>&1 || true
}

java_is_17() {
  local home="$1"
  local text
  text="$(java_major_text "$home")" || return 1
  printf '%s\n' "$text" | grep -q "openjdk 17"
}

jdk_asset() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "${os}/${arch}" in
    Darwin/arm64)
      JDK_TARBALL="OpenJDK17U-jdk_aarch64_mac_hotspot_17.0.21_9.tar.gz"
      JDK_URL="https://github.com/adoptium/temurin17-binaries/releases/download/jdk-17.0.21%2B9/${JDK_TARBALL}"
      JDK_SHA256="78b08c7f5a0598242d5719c022e19479cb4a3aa45c37c19726a757a469ed1c15"
      ;;
    Darwin/x86_64)
      JDK_TARBALL="OpenJDK17U-jdk_x64_mac_hotspot_17.0.21_9.tar.gz"
      JDK_URL="https://github.com/adoptium/temurin17-binaries/releases/download/jdk-17.0.21%2B9/${JDK_TARBALL}"
      JDK_SHA256="868f5c672b3389fdbe48e44f94ae170f01668b1401360944fc7159d7cc9772cc"
      ;;
    *)
      die "закреплённый Temurin ${JDK_SEMVER} в скрипте есть для macOS arm64 и x86_64, сейчас ${os}/${arch}. Поставьте OpenJDK 17 и передайте JAVA_HOME. Глобальные настройки скрипт не меняет."
      ;;
  esac
}

ensure_jdk() {
  if [[ -n "${JAVA_HOME:-}" ]] && java_is_17 "$JAVA_HOME"; then
    echo "java: использую JAVA_HOME=${JAVA_HOME}"
    java_major_text "$JAVA_HOME" | head -n 1
    return
  fi
  if [[ -n "${JAVA_HOME:-}" ]]; then
    echo "java: JAVA_HOME=${JAVA_HOME} не OpenJDK 17, ищу закреплённый Temurin в ${CACHE}/jdk" >&2
  fi
  jdk_asset
  local dest="${CACHE}/jdk/${JDK_RELEASE}/Contents/Home"
  if [[ ! -x "${dest}/bin/java" ]]; then
    mkdir -p "${CACHE}/jdk"
    local tar="${CACHE}/jdk/${JDK_TARBALL}"
    echo "download: Temurin ${JDK_SEMVER}"
    if ! curl -fL --retry 3 -o "$tar" "$JDK_URL"; then
      rm -f "$tar"
      die "не удалось скачать ${JDK_URL}"
    fi
    local got
    got="$(shasum -a 256 "$tar" | awk '{print $1}')"
    if [[ "$got" != "$JDK_SHA256" ]]; then
      rm -f "$tar"
      die "sha256 Temurin не совпал: ${got}, ожидался ${JDK_SHA256}"
    fi
    tar -xzf "$tar" -C "${CACHE}/jdk"
  fi
  if ! java_is_17 "$dest"; then
    die "Temurin в ${dest} не сообщает openjdk 17."
  fi
  JAVA_HOME="$dest"
  export JAVA_HOME
  echo "java: ${JAVA_HOME}"
  java_major_text "$JAVA_HOME" | head -n 1
}

ensure_sdk() {
  local sdk="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$DEFAULT_SDK}}"
  if [[ ! -f "${sdk}/licenses/android-sdk-license" ]]; then
    die "Android SDK не найден в ${sdk}. Ожидается существующий SDK (${DEFAULT_SDK}). Скрипт sdkmanager не запускает и второй SDK не ставит."
  fi
  local ndk="${ANDROID_NDK_HOME:-${sdk}/ndk/${NDK_REVISION}}"
  if [[ ! -f "${ndk}/source.properties" ]]; then
    die "NDK не найден в ${ndk}. Нужна уже установленная ревизия ${NDK_REVISION}. Скрипт NDK не скачивает."
  fi
  if ! grep -F -q "Pkg.Revision = ${NDK_REVISION}" "${ndk}/source.properties"; then
    die "NDK в ${ndk} не равен закреплённой ревизии ${NDK_REVISION}. Скрипт другую ревизию не подставляет."
  fi
  if [[ ! -f "${sdk}/platforms/android-36/android.jar" && ! -f "${sdk}/platforms/android-35/android.jar" ]]; then
    die "в ${sdk}/platforms нет android-35 или android-36. Скрипт платформы не доустанавливает."
  fi
  ANDROID_SDK_ROOT="$sdk"
  ANDROID_HOME="$sdk"
  ANDROID_NDK_HOME="$ndk"
  export ANDROID_SDK_ROOT ANDROID_HOME ANDROID_NDK_HOME
  echo "sdk: ${ANDROID_SDK_ROOT}"
  echo "ndk: ${ANDROID_NDK_HOME}"
}

ensure_gomobile() {
  mkdir -p "$GOBIN" "$GOPATH_LOCAL" "$GOCACHE_LOCAL"
  local modcache
  modcache="$(go env GOMODCACHE)"
  export GOBIN GOPATH="${GOPATH_LOCAL}" GOMODCACHE="${modcache}" GOCACHE="${GOCACHE_LOCAL}"
  if [[ ! -x "${GOBIN}/gomobile" || ! -x "${GOBIN}/gobind" ]]; then
    echo "install: gomobile ${GOMOBILE_VERSION} -> ${GOBIN}"
    if ! env -u GOFLAGS GOTOOLCHAIN=local GOBIN="$GOBIN" GOPATH="$GOPATH_LOCAL" GOMODCACHE="$modcache" GOCACHE="$GOCACHE_LOCAL" \
      go install "github.com/sagernet/gomobile/cmd/gomobile@${GOMOBILE_VERSION}" \
                 "github.com/sagernet/gomobile/cmd/gobind@${GOMOBILE_VERSION}"; then
      die "не удалось поставить gomobile ${GOMOBILE_VERSION}. Сеть нужна только если модуля ещё нет в кэше."
    fi
  fi
  echo "gomobile: ${GOBIN}/gomobile ${GOMOBILE_VERSION}"
  # `gomobile init` в v0.1.13 ставит gobind@latest и нужен только для OpenAL.
  # bind проверяет лишь наличие каталога $GOPATH/pkg/gomobile. NDK берётся из ANDROID_NDK_HOME.
  mkdir -p "${GOPATH_LOCAL}/pkg/gomobile"
}

refresh_licenses() {
  if [[ ! -f "$SRC/LICENSE" ]]; then
    die "в ревизии нет LICENSE."
  fi
  install_file "$SRC/LICENSE" "${ROOT}/LICENSE"
  local listed ver dir
  if ! listed="$(
    cd "$SRC"
    env -u GOFLAGS GOTOOLCHAIN=local GOPATH="$GOPATH_LOCAL" GOMODCACHE="$(go env GOMODCACHE)" GOCACHE="$GOCACHE_LOCAL" \
      go list -m -f '{{.Version}} {{.Dir}}' "$MIERU_MODULE"
  )"; then
    die "не удалось найти модуль ${MIERU_MODULE}."
  fi
  ver="${listed%% *}"
  dir="${listed#* }"
  if [[ "$ver" != "$MIERU_VERSION" ]]; then
    die "модуль ${MIERU_MODULE} имеет версию ${ver}, ожидалась ${MIERU_VERSION}."
  fi
  if [[ ! -f "${dir}/LICENSE" ]]; then
    die "в модуле ${MIERU_MODULE}@${ver} нет LICENSE."
  fi
  install_file "${dir}/LICENSE" "${DIST}/licenses/mieru-${MIERU_VERSION}.LICENSE"
  install_file "${ROOT}/LICENSE" "${DIST}/LICENSE"
  echo "license: ${MIERU_MODULE}@${ver}"
}

build_aar() {
  local tmp
  mkdir -p "$DIST"
  tmp="$(mktemp "${DIST}/.libbox.XXXXXX.aar")"
  local ldflags="-X github.com/sagernet/sing-box/constant.Version=${MBOX_VERSION} -X runtime.godebugDefault=multipathtcp=0,tlssha1=1 -checklinkname=0 -s -w -buildid="
  echo "build: aar android/arm64,android/amd64 api ${ANDROID_API} -> ${AAR}"
  if ! (
    cd "$SRC"
    env -u GOFLAGS \
      GOTOOLCHAIN=local \
      JAVA_HOME="$JAVA_HOME" \
      ANDROID_HOME="$ANDROID_HOME" \
      ANDROID_SDK_ROOT="$ANDROID_SDK_ROOT" \
      ANDROID_NDK_HOME="$ANDROID_NDK_HOME" \
      GOPATH="$GOPATH_LOCAL" \
      GOMODCACHE="$(go env GOMODCACHE)" \
      GOCACHE="$GOCACHE_LOCAL" \
      PATH="${JAVA_HOME}/bin:${GOBIN}:${PATH}" \
      "${GOBIN}/gomobile" bind -v \
        -target "android/arm64,android/amd64" \
        -androidapi "$ANDROID_API" \
        -javapkg="$JAVAPKG" \
        -libname="$LIBNAME" \
        -trimpath \
        -buildvcs=false \
        -ldflags "$ldflags" \
        -tags "$BUILD_TAGS" \
        -o "$tmp" \
        ./experimental/libbox
  ); then
    rm -f "$tmp"
    die "gomobile bind завершился с ошибкой. Вывод выше. AAR-заглушка не создаётся."
  fi
  mv "$tmp" "$AAR"
  chmod 644 "$AAR"
}

assert_clean_source() {
  local dirty
  dirty="$(git -C "$SRC" status --porcelain)"
  if [[ -n "$dirty" ]]; then
    printf '%s\n' "$dirty" >&2
    die "сборка изменила клон ${SRC}. Закреплённый commit должен остаться чистым."
  fi
}

verify_aar() {
  local work so desc classes listing
  work="$(mktemp -d "${TMPDIR:-/tmp}/libbox-aar.XXXXXX")"
  unzip -q "$AAR" -d "$work"
  for so in \
    "${work}/jni/arm64-v8a/libbox.so" \
    "${work}/jni/x86_64/libbox.so"
  do
    if [[ ! -f "$so" ]]; then
      die "в AAR нет ${so#${work}/}"
    fi
    desc="$(file -b "$so")"
    echo "${so#${work}/}: ${desc}"
  done
  case "$(file -b "${work}/jni/arm64-v8a/libbox.so")" in
    "ELF 64-bit LSB shared object, ARM aarch64,"*) ;;
    *) die "arm64-v8a/libbox.so не ELF aarch64" ;;
  esac
  case "$(file -b "${work}/jni/x86_64/libbox.so")" in
    "ELF 64-bit LSB shared object, x86-64,"*) ;;
    *) die "x86_64/libbox.so не ELF x86-64" ;;
  esac
  if find "${work}/jni" -type d \( -name 'armeabi-v7a' -o -name 'x86' \) | grep -q .; then
    die "AAR содержит лишнюю ABI. Нужны только arm64-v8a и x86_64."
  fi
  if ! grep -F -q 'minSdkVersion="26"' "${work}/AndroidManifest.xml"; then
    echo "manifest:" >&2
    cat "${work}/AndroidManifest.xml" >&2
    die "AndroidManifest не закрепляет minSdkVersion 26."
  fi
  # macOS strings не читает строки из ELF .rodata. Ищем по сырым байтам.
  for so in "${work}/jni/arm64-v8a/libbox.so" "${work}/jni/x86_64/libbox.so"; do
    if ! grep -a -F -q "mieru" "$so"; then
      die "${so#${work}/} не содержит строку mieru. Протокол в эту сборку не попал."
    fi
    if ! grep -a -F -q "${MBOX_VERSION}" "$so"; then
      die "${so#${work}/} не содержит версию ${MBOX_VERSION}."
    fi
  done
  classes="${work}/classes.jar"
  if [[ ! -f "$classes" ]]; then
    die "в AAR нет classes.jar."
  fi
  listing="$(unzip -l "$classes")"
  printf '%s\n' "$listing" | grep -F "io/nekohasekai/libbox/Libbox.class" >/dev/null
  printf '%s\n' "$listing" | grep -F "io/nekohasekai/libbox/CommandServer.class" >/dev/null
  printf '%s\n' "$listing" | grep -F "io/nekohasekai/libbox/PlatformInterface.class" >/dev/null
  local javap="${JAVA_HOME}/bin/javap"
  local api="${work}/javap.txt"
  {
    echo "===== Libbox ====="
    "$javap" -public -classpath "$classes" "${JAVA_PACKAGE}.Libbox"
    echo "===== CommandServer ====="
    "$javap" -public -classpath "$classes" "${JAVA_PACKAGE}.CommandServer"
    echo "===== PlatformInterface ====="
    "$javap" -public -classpath "$classes" "${JAVA_PACKAGE}.PlatformInterface"
    echo "===== SetupOptions ====="
    "$javap" -public -classpath "$classes" "${JAVA_PACKAGE}.SetupOptions"
    echo "===== OverrideOptions ====="
    "$javap" -public -classpath "$classes" "${JAVA_PACKAGE}.OverrideOptions"
    echo "===== TunOptions ====="
    "$javap" -public -classpath "$classes" "${JAVA_PACKAGE}.TunOptions"
    echo "===== CommandServerHandler ====="
    "$javap" -public -classpath "$classes" "${JAVA_PACKAGE}.CommandServerHandler"
  } > "$api"
  cp "$api" "${DIST}/libbox-javap.txt"
  local need
  for need in \
    "setup(io.nekohasekai.libbox.SetupOptions)" \
    "newCommandServer(io.nekohasekai.libbox.CommandServerHandler, io.nekohasekai.libbox.PlatformInterface)" \
    "startOrReloadService(java.lang.String, io.nekohasekai.libbox.OverrideOptions)" \
    "closeService()" \
    "openTun(io.nekohasekai.libbox.TunOptions)" \
    "autoDetectInterfaceControl(int)"
  do
    if ! grep -F -q "$need" "$api"; then
      echo "javap:" >&2
      cat "$api" >&2
      die "в сгенерированных bindings нет сигнатуры: ${need}"
    fi
  done
  echo "api: bindings содержат Setup, NewCommandServer, StartOrReloadService, CloseService, OpenTun, AutoDetectInterfaceControl"
  rm -rf "$work"
}

write_revision() {
  cat > "${DIST}/android-REVISION" <<EOF
repository=https://github.com/enfein/mbox
branch=${MBOX_BRANCH}
commit=${MBOX_COMMIT}
url=${MBOX_URL}
module=github.com/sagernet/sing-box
version=${MBOX_VERSION}
mieru=${MIERU_MODULE} ${MIERU_VERSION}
artifact=libbox.aar
javapkg=${JAVA_PACKAGE}
libname=${LIBNAME}
android_api=${ANDROID_API}
abis=arm64-v8a,x86_64
tags=${BUILD_TAGS}
gomobile=${GOMOBILE_VERSION}
jdk=${JDK_SEMVER}
jdk_home=${JAVA_HOME}
sdk=${ANDROID_SDK_ROOT}
ndk=${NDK_REVISION}
ndk_path=${ANDROID_NDK_HOME}
cgo=enabled
buildvcs=false
EOF
}

main() {
  case "${1:-}" in
    "") ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      die "неизвестный аргумент: $1. Запустите без аргументов или с --help."
      ;;
  esac

  need git "Нужен git, чтобы проверить commit ${MBOX_COMMIT}."
  need go "Поставьте Go ${MIN_GO} или новее: https://go.dev/dl/."
  need file "Нужен file, чтобы подтвердить ELF обеих ABI."
  need curl "Нужен curl, чтобы скачать закреплённый Temurin, если его ещё нет в ${CACHE}/jdk."
  need tar "Нужен tar, чтобы распаковать Temurin."
  need unzip "Нужен unzip, чтобы проверить содержимое AAR."
  check_go
  ensure_source
  ensure_jdk
  ensure_sdk
  ensure_gomobile
  build_aar
  assert_clean_source
  refresh_licenses
  verify_aar
  write_revision
  cat <<EOF
ok: ${AAR}
artifacts:
  ${AAR}
  ${DIST}/android-REVISION
  ${DIST}/libbox-javap.txt
  ${DIST}/LICENSE
  ${DIST}/licenses/mieru-${MIERU_VERSION}.LICENSE
EOF
}

main "$@"
