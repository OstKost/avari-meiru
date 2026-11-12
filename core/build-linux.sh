#!/bin/bash
# Сборка sidecar Linux x86_64 из закреплённой ревизии enfein/mbox.
# Скрипт не вызывает `sing-box run`, не открывает TUN и не меняет маршруты.
# macOS-артефакты build-desktop.sh не удаляет и не пересобирает.
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
CHECK_CONFIG="${ROOT}/check-loopback.json"
DIST="${ROOT}/dist"
SRC="${MBOX_SRC:-${ROOT}/.cache/mbox}"
TRIPLE="x86_64-unknown-linux-gnu"
GOARCH_TARGET="amd64"
BIN_NAME="sing-box-${TRIPLE}"

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  cat <<EOF
usage: ./build-linux.sh

Клонирует ${MBOX_REPO} (ветка ${MBOX_BRANCH}), проверяет commit
${MBOX_COMMIT} и собирает sidecar Linux x86_64:

  ${DIST}/${BIN_NAME}

Теги: ${BUILD_TAGS}. CGO_ENABLED=0, GOTOOLCHAIN=local.
sing-box run не вызывается. macOS sidecar этот скрипт не трогает.

Если доступен Linux x86_64 (этот хост или запущенный docker),
выполняет sing-box version и sing-box check. Если runtime нет,
сборка и проверка file всё равно должны завершиться с кодом 0,
а отсутствие check печатается как warning.

Переменные:
  MBOX_SRC   готовый чистый клон с HEAD=${MBOX_COMMIT}.
             Иначе исходники клонируются в ${ROOT}/.cache/mbox.
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
    die "не удалось прочитать версию Go (go env GOVERSION вернул «${raw:-пусто}»). Нужен Go ${MIN_GO} или новее. Скрипт ставит GOTOOLCHAIN=local и сам toolchain не скачивает."
  fi
  ver="${ver%%-*}"
  if ! go_version_at_least "$ver" "$MIN_GO"; then
    die "установлен Go ${ver}, а go.mod закреплённой ревизии требует ${MIN_GO} или новее. Поставьте более новый Go с https://go.dev/dl/. GOTOOLCHAIN=local запрещает автоматическую загрузку toolchain."
  fi
  echo "go: ${ver}"
}

ensure_source() {
  local head dirty
  if [[ -d "$SRC" ]]; then
    if [[ ! -e "$SRC/.git" ]]; then
      die "каталог исходников ${SRC} уже есть, но это не git-клон. Укажите другой MBOX_SRC или удалите каталог и запустите скрипт снова."
    fi
    if ! head="$(git -C "$SRC" rev-parse HEAD 2>/dev/null)"; then
      die "не удалось прочитать HEAD в ${SRC}. Каталог повреждён. Удалите его и запустите скрипт снова."
    fi
    if [[ "$head" != "$MBOX_COMMIT" ]]; then
      die "в ${SRC} ревизия ${head}, нужна ${MBOX_COMMIT} (${MBOX_URL}). Скрипт чужой клон не переключает. Уберите MBOX_SRC или удалите ${SRC} и запустите снова."
    fi
    dirty="$(git -C "$SRC" status --porcelain)"
    if [[ -n "$dirty" ]]; then
      die "клон ${SRC} содержит незакоммиченные изменения. Сборка должна совпадать с ${MBOX_COMMIT}. Очистите дерево или используйте чистый клон."
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
    die "не удалось клонировать ${MBOX_REPO} (ветка ${MBOX_BRANCH}). Проверьте git, сеть и доступ к GitHub. Частичный каталог: ${SRC}."
  fi
  if ! head="$(git -C "$SRC" rev-parse HEAD 2>/dev/null)"; then
    die "клон ${SRC} не содержит HEAD. Удалите каталог и запустите скрипт снова."
  fi
  if [[ "$head" != "$MBOX_COMMIT" ]]; then
    die "ветка ${MBOX_BRANCH} указывает на ${head}, а не на закреплённый ${MBOX_COMMIT}. Сборка остановлена: скрипт ревизию сам не двигает."
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

refresh_mbox_license() {
  if [[ ! -f "$SRC/LICENSE" ]]; then
    die "в ревизии ${MBOX_COMMIT} нет файла LICENSE."
  fi
  install_file "$SRC/LICENSE" "${ROOT}/LICENSE"
}

copy_mieru_license() {
  local listed ver dir
  if ! listed="$(
    cd "$SRC"
    env -u GOFLAGS GOTOOLCHAIN=local go list -m -f '{{.Version}} {{.Dir}}' "$MIERU_MODULE"
  )"; then
    die "не удалось найти модуль ${MIERU_MODULE}. Сначала должна пройти сборка, которой нужен этот модуль."
  fi
  ver="${listed%% *}"
  dir="${listed#* }"
  if [[ "$ver" != "$MIERU_VERSION" ]]; then
    die "модуль ${MIERU_MODULE} имеет версию ${ver}, ожидалась ${MIERU_VERSION}."
  fi
  if [[ ! -f "${dir}/LICENSE" ]]; then
    die "в модуле ${MIERU_MODULE}@${ver} нет LICENSE (${dir})."
  fi
  install_file "${dir}/LICENSE" "${DIST}/licenses/mieru-${MIERU_VERSION}.LICENSE"
  echo "license: ${MIERU_MODULE}@${ver}"
}

build_linux() {
  local out="${DIST}/${BIN_NAME}"
  local tmp
  mkdir -p "$DIST"
  tmp="$(mktemp "${DIST}/.sing-box.XXXXXX")"
  echo "build: linux/${GOARCH_TARGET} -> ${out}"
  if ! (
    cd "$SRC"
    env -u GOFLAGS \
      GOTOOLCHAIN=local \
      CGO_ENABLED=0 \
      GOOS=linux \
      GOARCH="$GOARCH_TARGET" \
      go build -trimpath -buildvcs=true \
        -ldflags "-X github.com/sagernet/sing-box/constant.Version=${MBOX_VERSION} -X runtime.godebugDefault=multipathtcp=0,tlssha1=1 -checklinkname=0 -s -w -buildid=" \
        -tags "$BUILD_TAGS" \
        -o "$tmp" ./cmd/sing-box
  ); then
    rm -f "$tmp"
    die "сборка linux/${GOARCH_TARGET} завершилась с ошибкой. Вывод go build выше. Нужны Go ${MIN_GO}+ и сеть, если модулей ещё нет в кэше. CGO выключен, with_naive_outbound не используется."
  fi
  mv "$tmp" "$out"
  chmod 755 "$out"
}

expect_elf() {
  local bin="$1"
  local desc
  desc="$(file -b "$bin")"
  echo "${bin}: ${desc}"
  case "$desc" in
    "ELF 64-bit LSB executable, x86-64,"*) ;;
    *)
      die "ожидался ELF 64-bit LSB executable, x86-64, file сообщает: ${desc}"
      ;;
  esac
}

require_version_text() {
  local text="$1"
  local label="$2"
  if ! printf '%s\n' "$text" | grep -F -q -- "sing-box version ${MBOX_VERSION}"; then
    die "${label}: нет строки «sing-box version ${MBOX_VERSION}». Вывод:"$'\n'"${text}"
  fi
  if ! printf '%s\n' "$text" | grep -F -q -- "linux/${GOARCH_TARGET}"; then
    die "${label}: бинарник собран не для linux/${GOARCH_TARGET}. Вывод:"$'\n'"${text}"
  fi
  if ! printf '%s\n' "$text" | grep -F -q -- "Tags: ${BUILD_TAGS}"; then
    die "${label}: теги не совпали с «${BUILD_TAGS}». Вывод:"$'\n'"${text}"
  fi
  if ! printf '%s\n' "$text" | grep -F -q -- "Revision: ${MBOX_COMMIT}"; then
    die "${label}: нет ревизии ${MBOX_COMMIT}. Сборка должна идти из git-клона с -buildvcs=true. Вывод:"$'\n'"${text}"
  fi
  if ! printf '%s\n' "$text" | grep -F -q -- "CGO: disabled"; then
    die "${label}: CGO должен быть выключен. Вывод:"$'\n'"${text}"
  fi
}

write_revision() {
  cat > "${DIST}/linux-REVISION" <<EOF
repository=https://github.com/enfein/mbox
branch=${MBOX_BRANCH}
commit=${MBOX_COMMIT}
url=${MBOX_URL}
module=github.com/sagernet/sing-box
version=${MBOX_VERSION}
mieru=${MIERU_MODULE} ${MIERU_VERSION}
goos=linux
goarch=${GOARCH_TARGET}
triple=${TRIPLE}
artifact=${BIN_NAME}
tags=${BUILD_TAGS}
cgo=disabled
EOF
  install_file "${ROOT}/LICENSE" "${DIST}/LICENSE"
}

run_check_cmd() {
  local bin="$1"
  shift
  local cfg text
  cfg="$(cd "$(dirname "$CHECK_CONFIG")" && pwd)/$(basename "$CHECK_CONFIG")"
  echo "version: $*"
  text="$("$@" version)"
  printf '%s\n' "$text"
  require_version_text "$text" "linux sidecar"
  echo "check: $* check -c ${cfg} --disable-color"
  if ! "$@" check -c "$cfg" --disable-color; then
    die "sing-box check не принял ${cfg}. Ненулевой код выше означает, что Linux sidecar к работе не готов. Скрипт не вызывает sing-box run."
  fi
  echo "check: принят ${cfg}"
  RAN_CHECK=1
}

try_runtime() {
  local bin="$1"
  local cfg os arch
  RAN_CHECK=0
  os="$(uname -s)"
  arch="$(uname -m)"
  cfg="$(cd "$(dirname "$CHECK_CONFIG")" && pwd)/$(basename "$CHECK_CONFIG")"
  if [[ "$os" == "Linux" && "$arch" == "x86_64" ]]; then
    run_check_cmd "$bin" "$bin"
    return
  fi
  if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    echo "runtime: docker --platform linux/amd64"
    if ! docker run --rm --platform linux/amd64 \
      -v "${bin}:/sing-box:ro" \
      -v "${cfg}:/check.json:ro" \
      ubuntu:24.04 \
      /sing-box version > "${DIST}/.linux-version.txt"; then
      die "не удалось выполнить ${bin} в контейнере ubuntu:24.04. Вывод docker выше."
    fi
    local text
    text="$(cat "${DIST}/.linux-version.txt")"
    rm -f "${DIST}/.linux-version.txt"
    printf '%s\n' "$text"
    require_version_text "$text" "linux sidecar"
    echo "check: docker ubuntu:24.04 /sing-box check -c /check.json --disable-color"
    if ! docker run --rm --platform linux/amd64 \
      -v "${bin}:/sing-box:ro" \
      -v "${cfg}:/check.json:ro" \
      ubuntu:24.04 \
      /sing-box check -c /check.json --disable-color; then
      die "sing-box check в ubuntu:24.04 не принял ${cfg}. Скрипт не вызывает sing-box run."
    fi
    echo "check: принят ${cfg}"
    RAN_CHECK=1
    return
  fi
  echo "warning: Linux runtime недоступен. ${bin} собран, file подтвердил ELF x86-64, но sing-box version и sing-box check на Linux не выполнялись. Запустите docker и повторите скрипт, чтобы прогнать check." >&2
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
  need go "Поставьте Go ${MIN_GO} или новее: https://go.dev/dl/. Скрипт использует GOTOOLCHAIN=local и не скачивает toolchain сам."
  need file "Нужен file, чтобы подтвердить ELF x86-64."
  check_go
  if [[ ! -f "$CHECK_CONFIG" ]]; then
    die "нет минимального конфига ${CHECK_CONFIG}."
  fi

  ensure_source
  refresh_mbox_license
  build_linux
  expect_elf "${DIST}/${BIN_NAME}"
  copy_mieru_license
  write_revision
  try_runtime "${DIST}/${BIN_NAME}"

  if [[ "$RAN_CHECK" == "1" ]]; then
    echo "ok: Linux sidecar ${TRIPLE} прошёл sing-box check."
  else
    echo "ok: Linux sidecar ${TRIPLE} собран. sing-box check на Linux не запускался: runtime нет."
  fi
  cat <<EOF
artifacts:
  ${DIST}/${BIN_NAME}
  ${DIST}/linux-REVISION
  ${DIST}/LICENSE
  ${DIST}/licenses/mieru-${MIERU_VERSION}.LICENSE
EOF
}

main "$@"
