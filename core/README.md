# Ядро

Закреплённый [`enfein/mbox`](https://github.com/enfein/mbox) commit `1c5b39718ebcd05e6adb31f95246fbf056be9bd7` (ветка `release-1.14.3`, в бинарнике версия `1.14.3`). Имя программы остаётся `sing-box`. Приложение называется своим именем и не представляется как sing-box.

Три сборки одной ревизии, одни теги `with_gvisor,badlinkname,tfogo_checklinkname0`:

| Скрипт | Артефакт |
|---|---|
| `./build-desktop.sh` | `core/dist/sing-box-aarch64-apple-darwin`, `core/dist/sing-box-x86_64-apple-darwin` |
| `./build-linux.sh` | `core/dist/sing-box-x86_64-unknown-linux-gnu` |
| `./build-android.sh` | `core/dist/libbox.aar` (`arm64-v8a`, `x86_64`, minSdk 26) |

Контракт поведения macOS sidecar: `docs/core-contract.md`. Контракт Android libbox: `ANDROID_CONTRACT.md` в этом каталоге. Windows amd64 этими скриптами не собирается.

Ни один скрипт не вызывает `sing-box run`, не открывает TUN и не меняет маршруты машины. Штатная проверка sidecar — `sing-box check`. Android проверяется составом AAR и `javap`, без устройства.

## Toolchain

- macOS arm64 или amd64, чтобы выполнить локальный бинарник.
- `git`. Без него скрипт завершается с кодом 127.
- Go 1.25.5 или новее (`go.mod` этой ревизии). На хосте контракта стоит Go 1.27.3. Скрипт ставит `GOTOOLCHAIN=local` и сам toolchain не скачивает. Более старый Go — код 1.
- CGO выключен. Xcode для этой сборки не нужен.
- Сеть нужна для первого клона и для модулей Go, если их ещё нет в кэше модуля.
- `file` нужен, чтобы подтвердить Mach-O архитектуру обоих sidecar.

Теги сборки урезаны относительно `release/DEFAULT_BUILD_TAGS`. Полный список включает `with_naive_outbound` и на Apple требует CGO. Для mieru и TUN достаточно:

```text
with_gvisor,badlinkname,tfogo_checklinkname0
```

`with_gvisor` — стек TUN из контракта. `badlinkname` и `tfogo_checklinkname0` идут вместе с `-checklinkname=0` из `release/LDFLAGS`.

## Команды

Из каталога `core`:

```sh
./build-desktop.sh
```

Скрипт клонирует ветку `release-1.14.3` в `core/.cache/mbox` и останавливается, если `git rev-parse HEAD` не равен `1c5b39718ebcd05e6adb31f95246fbf056be9bd7`. Уже существующий чистый клон той же ревизии можно передать явно:

```sh
MBOX_SRC=/path/to/mbox ./build-desktop.sh
```

Чужой commit, грязное дерево, отсутствующий каталог и неизвестный аргумент дают код 1 и текст ошибки на stderr. Скрипт не переключает чужой клон и не двигает закрепление.

Команда сборки, которую выполняет скрипт из корня исходников. Для Apple Silicon `GOARCH=arm64`, для Intel `GOARCH=amd64`. `GOOS=darwin` в обоих случаях. Окружение `GOFLAGS` вызывающего процесса на сборку не влияет.

```sh
export GOTOOLCHAIN=local CGO_ENABLED=0 GOOS=darwin GOARCH=arm64
go build -trimpath -buildvcs=true \
  -ldflags "-X github.com/sagernet/sing-box/constant.Version=1.14.3 -X runtime.godebugDefault=multipathtcp=0,tlssha1=1 -checklinkname=0 -s -w -buildid=" \
  -tags "with_gvisor,badlinkname,tfogo_checklinkname0" \
  -o sing-box-aarch64-apple-darwin ./cmd/sing-box
```

`-buildvcs=true` записывает commit в бинарник. `sing-box version` должен показать `Revision: 1c5b39718ebcd05e6adb31f95246fbf056be9bd7`.

## Артефакты

Успешный код 0 оставляет:

| Путь | Что это |
|---|---|
| `core/dist/sing-box-aarch64-apple-darwin` | sidecar macOS Apple Silicon |
| `core/dist/sing-box-x86_64-apple-darwin` | sidecar macOS Intel |
| `core/dist/LICENSE` | `LICENSE` mbox этой ревизии |
| `core/dist/REVISION` | URL, commit, версия, модуль mieru, теги |
| `core/dist/licenses/mieru-v3.38.0.LICENSE` | GPL-3.0 модуля `github.com/enfein/mieru/v3 v3.38.0` |
| `core/LICENSE` | тот же текст `LICENSE` mbox, обновляется из клона |

`core/dist/` и `core/.cache/` перечислены в `core/.gitignore`. Большие бинарники и клон в репозиторий не кладутся. `check-loopback.json` — фикстура с фиктивными данными, не действующий профиль пользователя.

Стабильные пути для T4 и Tauri v2. В `desktop/src-tauri/tauri.conf.json` путь указывается без суффикса тройки, относительно каталога `desktop/src-tauri`:

```json
"externalBin": ["../../core/dist/sing-box"]
```

Tauri дописывает `-aarch64-apple-darwin` или `-x86_64-apple-darwin` и берёт файл из `core/dist/`. Перед сборкой приложения `./build-desktop.sh` должен завершиться с кодом 0. Имя файла — имя sidecar для Tauri; строка версии внутри по-прежнему `sing-box`.

`core/dist/REVISION` — пары `ключ=значение`: `repository`, `branch`, `commit`, `url`, `module`, `version`, `mieru`, `tags`, `cgo`. Linux пишет тот же набор в `core/dist/linux-REVISION` и не затирает macOS `REVISION`. Android пишет `core/dist/android-REVISION`: у AAR `-buildvcs=false`, поэтому commit лежит в этом файле, а не внутри `.so`.

## Linux x86_64

Из каталога `core`:

```sh
./build-linux.sh
```

Клон и проверка commit те же, что у desktop. `CGO_ENABLED=0`, `GOOS=linux`, `GOARCH=amd64`, `-buildvcs=true`. Скрипт не удаляет и не пересобирает macOS-бинарники.

Если хост — Linux x86_64 или запущен Docker, скрипт выполняет `sing-box version` и `sing-box check` фикстуры `check-loopback.json`. На этой машине проверка прошла в `ubuntu:24.04` с `--platform linux/amd64`. Если Docker не запущен, сборка и `file` всё равно могут завершиться с кодом 0, а пропущенный `check` печатается как warning. `check` не вызывает `Start()` и не открывает TUN.

## Android AAR

Контракт API, владение fd, protect и toolchain: `ANDROID_CONTRACT.md`.

```sh
./build-android.sh
```

JDK — Eclipse Temurin 17.0.21+9 в `core/.cache/jdk`. Java 21 не используется. SDK: `/Volumes/KingstonM2/Android/sdk`. NDK закреплён как `28.2.13676358` и должен уже стоять; скрипт `sdkmanager` не вызывает и вторую ревизию NDK не скачивает. `gomobile`/`gobind` — `v0.1.13` в `core/.cache/tools/bin`. Пакет классов — `io.nekohasekai.libbox`, нативная библиотека — `libbox.so`.

Успешный код 0 оставляет `core/dist/libbox.aar`, `core/dist/android-REVISION` и `core/dist/libbox-javap.txt`.

## Проверка конфигурации

Фикстура `core/check-loopback.json`: mixed-вход на `127.0.0.1:12080`, outbound `mieru` с `transport: TCP` и `route.auto_detect_interface`. Адрес `203.0.113.10` — документационный TEST-NET-3. Имя `baozi` и пароль `manlianpenfen` — пример из контракта, не секрет пользователя. `traffic_pattern` в фикстуре нет.

Это конфиг для `check`, не рабочий VPN-конфиг. TUN, DNS и исключение адреса сервера приложение собирает само по `docs/core-contract.md`.

Скрипт для бинарника текущего Mac выполняет, с абсолютным путём:

```sh
./dist/sing-box-aarch64-apple-darwin check -c /absolute/path/to/core/check-loopback.json --disable-color
```

На Intel-хосте та же команда берёт `sing-box-x86_64-apple-darwin`. Код 0 значит, что ядро создало сервис из конфига и сразу закрыло его. `Start()` при этом не вызывается, TUN не открывается, доступность сервера не проверяется. Ненулевой код пишет на stderr строку `FATAL` и означает, что sidecar к запуску не готов.

Пароль передаётся только внутри файла. В аргументы и в эту инструкцию сверх уже записанной фикстуры он не добавляется.

## Если инструмента нет

| Ситуация | Код | Что печатается |
|---|---|---|
| нет `git` | 127 | как поставить Xcode Command Line Tools |
| нет `go` | 127 | как поставить Go и что toolchain сам не скачается |
| нет `file` | 127 | что без него нельзя подтвердить архитектуру |
| Go старее 1.25.5 | 1 | фактическая версия и требование `go.mod` |
| не macOS | 1 | Windows и Android этим скриптом не собираются |
| commit не совпал или дерево грязное | 1 | фактический hash и ожидаемый `1c5b39718ebcd05e6adb31f95246fbf056be9bd7` |
| `go build` не прошёл | 1 | вывод Go и напоминание про сеть и кэш модулей |
| `sing-box check` не прошёл | 1 | вывод `check`; `run` скрипт после этого не вызывает |

Кросс-сборка второй архитектуры macOS идёт той же командой с `CGO_ENABLED=0`. Отдельный macOS SDK для неё не нужен. Если хост не смог выполнить чужой бинарник, скрипт всё равно требует успешный `go build` и строку `Mach-O` от `file`. В этом случае запуск чужой архитектуры в выводе помечен как не проверенный.

## Проверено на этом хосте

Хост: macOS arm64, Go 1.27.3, 2026-11-11. `sing-box run` не запускался.

`./build-desktop.sh` с нуля клонировал `release-1.14.3` в `core/.cache/mbox`. `git rev-parse HEAD` равен `1c5b39718ebcd05e6adb31f95246fbf056be9bd7`. Повтор с `MBOX_SRC=/tmp/mbox-1.14.3` и повтор по клону в `.cache/mbox` тоже завершились с кодом 0. Отсутствие `go` даёт код 127. Чужой commit и грязный клон дают код 1 до сборки.

Оба бинарника выполнились здесь. `file` показывает Mach-O arm64 и Mach-O x86_64. Общие строки версии:

```text
sing-box version 1.14.3
Tags: with_gvisor,badlinkname,tfogo_checklinkname0
Revision: 1c5b39718ebcd05e6adb31f95246fbf056be9bd7
CGO: disabled
```

У arm64-бинарника окружение `go1.27.3 darwin/arm64`, у amd64 — `go1.27.3 darwin/amd64`. `sing-box check -c core/check-loopback.json --disable-color` вернул код 0 на обоих. Конфиг с пустым `password` на arm64-бинарнике вернул код 1 и stderr `FATAL[0000] ... password is empty`.

## Лицензии

Оба дерева — GPL-3.0-or-later.

- mbox: `core/LICENSE` и `core/dist/LICENSE`. К GPL добавлено условие: производная работа не должна использовать имя приложения или намекать на связь с ним без согласия. Рядом лежит `core/dist/REVISION` с URL и commit.
- mieru `v3.38.0`: полный текст GPL-3.0 скрипт копирует из модуля в `core/dist/licenses/mieru-v3.38.0.LICENSE`. Исходники модуля остаются в кэше Go по записи `go.sum` этой ревизии.

Отдельного юридического заключения этот каталог не делает.

## Что эта поставка не подтверждает

- Windows amd64 не собирался.
- Сборка с полным `DEFAULT_BUILD_TAGS` и с `with_naive_outbound` не выполнялась.
- `sing-box run` от root, интерфейс `utun`, установка и снятие маршрутов, живой сервер mieru.
- Android AAR собран и проверен по ABI и `javap`. Установка в приложение, `VpnService` на устройстве или эмуляторе и прохождение трафика не выполнялись. Подробности — в `ANDROID_CONTRACT.md`.
