#!/bin/bash
# Сборка sidecar macOS из закреплённой ревизии enfein/mbox.
# Скрипт не вызывает `sing-box run`, не открывает TUN и не меняет маршруты.
# Windows и Android здесь не собираются.
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

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  cat <<EOF
usage: ./build-desktop.sh

Клонирует ${MBOX_REPO} (ветка ${MBOX_BRANCH}), проверяет commit
${MBOX_COMMIT} и собирает sidecar macOS:

  ${DIST}/sing-box-aarch64-apple-darwin
  ${DIST}/sing-box-x86_64-apple-darwin

Затем для бинарника текущего Mac выполняет sing-box check
на ${CHECK_CONFIG}. sing-box run не вызывается.

Переменные:
  MBOX_SRC   готовый чистый клон с HEAD=${MBOX_COMMIT}.
             Иначе исходники клонируются в ${ROOT}/.cache/mbox.

Нужны git и Go ${MIN_GO}+. GOTOOLCHAIN=local, CGO_ENABLED=0.
Полный набор тегов mbox, Windows и Android AAR этот скрипт не собирает.
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

require_macos() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "${os}/${arch}" in
    Darwin/arm64)
      HOST_GOARCH="arm64"
      HOST_TRIPLE="aarch64-apple-darwin"
      ;;
    Darwin/x86_64)
      HOST_GOARCH="amd64"
      HOST_TRIPLE="x86_64-apple-darwin"
      ;;
    *)
      die "локальная сборка sidecar рассчитана на macOS arm64 или amd64, сейчас ${os}/${arch}. Windows и Android этот скрипт намеренно не собирает."
      ;;
  esac
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
  # Модульный кэш Go отдаёт LICENSE как 0444. Повторный обычный cp не может его перезаписать.
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

build_one() {
  local goarch="$1"
  local triple="$2"
  local out="${DIST}/sing-box-${triple}"
  local tmp
  mkdir -p "$DIST"
  tmp="$(mktemp "${DIST}/.sing-box.XXXXXX")"
  echo "build: darwin/${goarch} -> ${out}"
  if ! (
    cd "$SRC"
    # GOFLAGS вызывающего окружения не наследуем: артефакт должен совпадать с контрактом.
    env -u GOFLAGS \
      GOTOOLCHAIN=local \
      CGO_ENABLED=0 \
      GOOS=darwin \
      GOARCH="$goarch" \
      go build -trimpath -buildvcs=true \
        -ldflags "-X github.com/sagernet/sing-box/constant.Version=${MBOX_VERSION} -X runtime.godebugDefault=multipathtcp=0,tlssha1=1 -checklinkname=0 -s -w -buildid=" \
        -tags "$BUILD_TAGS" \
        -o "$tmp" ./cmd/sing-box
  ); then
    rm -f "$tmp"
    die "сборка darwin/${goarch} завершилась с ошибкой. Вывод go build выше. Нужны Go ${MIN_GO}+ и сеть, если модулей ещё нет в кэше. Xcode для этих тегов не нужен: CGO выключен, with_naive_outbound не используется."
  fi
  mv "$tmp" "$out"
  chmod 755 "$out"
}

expect_macho() {
  local bin="$1"
  local arch="$2"
  local desc
  desc="$(file -b "$bin")"
  echo "${bin}: ${desc}"
  case "$desc" in
    *"Mach-O 64-bit executable ${arch}"*) ;;
    *)
      die "ожидался Mach-O 64-bit executable ${arch}, file сообщает: ${desc}"
      ;;
  esac
}

require_version_text() {
  local text="$1"
  local goarch="$2"
  local label="$3"
  if ! printf '%s\n' "$text" | grep -F -q -- "sing-box version ${MBOX_VERSION}"; then
    die "${label}: нет строки «sing-box version ${MBOX_VERSION}». Вывод:"$'\n'"${text}"
  fi
  if ! printf '%s\n' "$text" | grep -F -q -- "darwin/${goarch}"; then
    die "${label}: бинарник собран не для darwin/${goarch}. Вывод:"$'\n'"${text}"
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

write_revision() {
  cat > "${DIST}/REVISION" <<EOF
repository=https://github.com/enfein/mbox
branch=${MBOX_BRANCH}
commit=${MBOX_COMMIT}
url=${MBOX_URL}
module=github.com/sagernet/sing-box
version=${MBOX_VERSION}
mieru=${MIERU_MODULE} ${MIERU_VERSION}
tags=${BUILD_TAGS}
cgo=disabled
EOF
  install_file "${ROOT}/LICENSE" "${DIST}/LICENSE"
}

run_host_check() {
  local bin="${DIST}/sing-box-${HOST_TRIPLE}"
  local cfg text
  cfg="$(cd "$(dirname "$CHECK_CONFIG")" && pwd)/$(basename "$CHECK_CONFIG")"
  text="$("$bin" version)"
  printf '%s\n' "$text"
  require_version_text "$text" "$HOST_GOARCH" "локальный sidecar"
  echo "check: ${bin} check -c ${cfg} --disable-color"
  if ! "$bin" check -c "$cfg" --disable-color; then
    die "sing-box check не принял ${cfg}. Ненулевой код выше означает, что локальный sidecar к работе не готов. Скрипт не вызывает sing-box run."
  fi
  echo "check: принят ${cfg}"
}

note_other() {
  local triple="$1"
  local goarch="$2"
  local bin="${DIST}/sing-box-${triple}"
  local text
  if text="$("$bin" version 2>&1)"; then
    printf '%s\n' "$text"
    require_version_text "$text" "$goarch" "$triple"
    echo "run: ${triple} выполнился на этом хосте"
  else
    echo "warning: ${bin} собран, архитектуру подтвердил file, но этот хост его не выполнил. Запуск этой архитектуры не проверен." >&2
  fi
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

  need git "Поставьте инструменты командной строки Xcode: xcode-select --install. Без git скрипт не может получить ревизию ${MBOX_COMMIT}."
  need go "Поставьте Go ${MIN_GO} или новее: https://go.dev/dl/. Скрипт использует GOTOOLCHAIN=local и не скачивает toolchain сам."
  need file "На macOS file входит в систему (/usr/bin/file) и нужен, чтобы подтвердить архитектуру sidecar."
  check_go
  require_macos
  if [[ ! -f "$CHECK_CONFIG" ]]; then
    die "нет минимального конфига ${CHECK_CONFIG}."
  fi

  ensure_source
  refresh_mbox_license

  build_one arm64 aarch64-apple-darwin
  build_one amd64 x86_64-apple-darwin

  expect_macho "${DIST}/sing-box-aarch64-apple-darwin" arm64
  expect_macho "${DIST}/sing-box-x86_64-apple-darwin" x86_64

  copy_mieru_license
  write_revision
  run_host_check

  if [[ "$HOST_TRIPLE" == "aarch64-apple-darwin" ]]; then
    note_other x86_64-apple-darwin amd64
  else
    note_other aarch64-apple-darwin arm64
  fi

  cat <<EOF
ok: локальный sidecar ${HOST_TRIPLE} прошёл sing-box check.
artifacts:
  ${DIST}/sing-box-aarch64-apple-darwin
  ${DIST}/sing-box-x86_64-apple-darwin
  ${DIST}/LICENSE
  ${DIST}/REVISION
  ${DIST}/licenses/mieru-${MIERU_VERSION}.LICENSE
EOF
}

main "$@"
