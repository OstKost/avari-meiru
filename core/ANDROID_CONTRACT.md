# Контракт libbox — Android

Дата проверки: 2026-11-11. Хост сборки: macOS arm64, Go 1.27.3. Живой VPN, эмулятор и устройство не запускались. `sing-box run` и root TUN не вызывались.

Это контракт для приложения, которое встраивает готовый `libbox.aar`. Исходники mbox не патчить. Общий SDK и NDK уже стоят; ставить их второй раз не нужно.

## Ревизия

| Поле | Значение |
|---|---|
| Репозиторий | https://github.com/enfein/mbox |
| Ветка | `release-1.14.3` |
| Commit | `1c5b39718ebcd05e6adb31f95246fbf056be9bd7` |
| Модуль | `github.com/sagernet/sing-box` |
| Версия в бинарнике | `1.14.3` |
| mieru | `github.com/enfein/mieru/v3 v3.38.0`, регистрация безусловная в `include/registry.go` |
| Теги этой сборки | `with_gvisor,badlinkname,tfogo_checklinkname0` |
| sing-tun | `github.com/sagernet/sing-tun v0.9.7-0.20261009020158-bca4bf029237` |

Commit в `.so` нет: `gomobile bind` вызван с `-buildvcs=false`. `Libbox.version()` возвращает `1.14.3`. Commit записан в `core/dist/android-REVISION`.

## Артефакт

Готовый файл:

```text
core/dist/libbox.aar
```

Абсолютный путь на этой машине: `/Volumes/KingstonM2/Projects/meiru-client/core/dist/libbox.aar`.

| Что внутри | Факт |
|---|---|
| `AndroidManifest.xml` | `package="go.libbox.gojni"`, `android:minSdkVersion="26"` |
| Классы | `classes.jar`, пакет `io.nekohasekai.libbox` |
| Нативная библиотека | `libbox.so`, имя из `-libname=box` |
| `jni/arm64-v8a/libbox.so` | ELF 64-bit ARM aarch64, dynamically linked, stripped |
| `jni/x86_64/libbox.so` | ELF 64-bit x86-64, dynamically linked, stripped |

`armeabi-v7a` и `x86` в архиве нет. Оба `.so` содержат байтовые строки `mieru`, `1.14.3` и `with_gvisor`. Строка `OpenVPN is not included in this build, rebuild with -tags with_openvpn` тоже есть: это заглушка `include/openvpn_stub.go` (`//go:build !with_openvpn`), не включённый тег. Рядом в бинарнике лежат подстроки `with_quic`, `with_naive_outbound`, `with_wireguard`, `with_tailscale`, `with_utls`: это тексты ошибок и импорты файлов без build-тега. В команду сборки они не передавались. `tailscale`, `utls` и `wireguard-go` всё равно попадают в `.so`, потому что часть `libbox` импортирует их без тега.

Рядом:

| Путь | Что это |
|---|---|
| `core/dist/android-REVISION` | commit, теги, JDK, SDK, NDK, gomobile |
| `core/dist/libbox-javap.txt` | `javap` классов, которыми скрипт проверяет AAR |
| `core/dist/LICENSE` | GPL-3.0-or-later mbox |
| `core/dist/licenses/mieru-v3.38.0.LICENSE` | GPL-3.0 mieru v3.38.0 |

Приложение называется своим именем и не представляется как sing-box. К GPL mbox добавлено условие: производная работа не должна использовать имя приложения или намекать на связь с ним без согласия.

## Toolchain

Ставить SDK, NDK и JDK заново не нужно и нельзя дублировать установку.

| Инструмент | Где | Примечание |
|---|---|---|
| Android SDK | `/Volumes/KingstonM2/Android/sdk` | platforms android-35 и android-36, build-tools 36.0.0. `sdkmanager` этот скрипт не вызывает |
| NDK | `/Volumes/KingstonM2/Android/sdk/ndk/28.2.13676358` | закреплено. Upstream предпочитает `28.0.13004108` и иначе предупреждает. Вторую ревизию не скачивать |
| JDK | `core/.cache/jdk/jdk-17.0.21+9/Contents/Home` | Eclipse Temurin 17.0.21+9. `java --version` содержит `openjdk 17.0.21` |
| gomobile, gobind | `core/.cache/tools/bin`, версия `v0.1.13` | `gomobile init` не вызывать: в v0.1.13 он ставит `gobind@latest` |
| Go | 1.27.3, `GOTOOLCHAIN=local` | `go.mod` ревизии требует 1.25.5 |
| Модули | `GOMODCACHE=/Volumes/KingstonM2/caches/go/pkg/mod` | не переносить |

Системный `java_home` пуст. Homebrew OpenJDK 21 для этой сборки не годится: проверка upstream ищет подстроку `openjdk 17`. `JAVA_HOME` для повтора сборки — каталог Temurin выше. Глобальные `JAVA_HOME`, `ANDROID_HOME` и настройки SDK скрипт не записывает.

Повтор:

```sh
cd core
./build-android.sh
```

Скрипт не вызывает `sing-box run`, не открывает TUN и при неуспехе не подменяет AAR заглушкой.

## Подтверждённые вызовы

Снято `javap` с `classes.jar` этого AAR.

```text
public static native void setup(SetupOptions) throws Exception;
public static native CommandServer newCommandServer(CommandServerHandler, PlatformInterface) throws Exception;
public native void startOrReloadService(String, OverrideOptions) throws Exception;
public native void closeService() throws Exception;
public abstract int openTun(TunOptions) throws Exception;
public abstract void autoDetectInterfaceControl(int) throws Exception;
public abstract boolean usePlatformAutoDetectInterfaceControl();
```

Пакет всех этих типов: `io.nekohasekai.libbox`.

`SetupOptions` — класс, не record. Поля, которые нужны старту:

| Метод | Тип Java | Почему так |
|---|---|---|
| `setBasePath` / `getBasePath` | `String` | каталог сокета команды |
| `setWorkingPath` / `getWorkingPath` | `String` | логи и `CrashReport-*.log` |
| `setTempPath` / `getTempPath` | `String` | временные файлы |
| `setFixAndroidStack` | `boolean` | Go `bool` |
| `setCommandServerListenPort` | `int` | Go-поле `int32` |
| `setLogMaxLines` | `long` | gomobile отображает Go `int` в Java `long` |
| `setDebug` | `boolean` | |
| `setCrashReportSource` | `String` | суффикс имени crash-лога |

`setFixAndroidStack(true)` нужен на Android: в `setup.go` это обход https://github.com/golang/go/issues/68760.

`OverrideOptions` обязателен и не может быть `null`: `StartOrReloadService` читает `options.AutoRedirect` без проверки. Итераторы пакетов могут быть `null` — `iteratorToArray` возвращает `nil`.

```text
void setAutoRedirect(boolean);
void setIncludePackage(StringIterator);
void setExcludePackage(StringIterator);
```

`setAutoRedirect(false)`. `auto_redirect` на Linux поднимает kernel auto-redirect (`tun.NewAutoRedirect`, nftables). На Android обёртка `UsePlatformInterface` всегда `true`, маршруты ставит `VpnService`, а не ядро. Флаг для этого пути не включать.

## Жизненный цикл

1. `Libbox.setup(options)` один раз. Создаёт `WorkingPath` и `TempPath` (`os.MkdirAll`, режим `0777`) и перенаправляет stderr в `WorkingPath/CrashReport-<CrashReportSource>.log`.
2. `Libbox.newCommandServer(handler, platform)`.
3. `CommandServer.start()` нужен только сокету команды. Порт `0` слушает unix `$BasePath/command.sock`. Ненулевой порт слушает `tcp 127.0.0.1:<port>`. Прямые вызовы из процесса сокет не требуют.
4. `startOrReloadService(configJson, overrideOptions)` разбирает JSON, стартует или перезагружает box и внутри вызывает `openTun`.
5. `closeService()` останавливает box и закрывает дубликат TUN fd, которым владеет sing-tun.
6. `close()` останавливает gRPC command server и рекордеры. Исходный TUN fd приложение закрывает само.

Порядок остановки: `closeService()`, затем закрыть сохранённый исходный fd, затем `close()`.

## Владение TUN fd

`platformInterfaceWrapper.UsePlatformInterface` в этой ревизии возвращает `true`. Android идёт через fd, не через `/dev/net/tun`.

`OpenInterface` (`experimental/libbox/service.go`):

1. `BuildAutoRouteRanges(true)`.
2. `OpenTun`. Возвращённый `int` — исходный fd.
3. `TUNGETIFF` (`getTunnelName` в `tun_name_linux.go`) по этому fd.
4. `syscall.Dup` (`service_other.go`). Дубликат пишется в `options.FileDescriptor`.
5. `tun.New` оборачивает дубликат в `os.NewFile`.

Правила для приложения:

- `openTun` возвращает fd из `VpnService.Builder.establish()`. Не закрывать его до возврата из `openTun`: dup выполняется в Go после возврата из Java.
- Сохранить fd в поле сервиса до `return`.
- libbox исходный fd не закрывает. Это верно и когда `TUNGETIFF` или `dup` падают уже после `openTun`.
- `NativeTun.Close` закрывает только `os.File` дубликата. При `FileDescriptor > 0` методы `unsetRoute`, `unsetRules` и `unsetAddresses` в `tun_linux.go` ничего не делают.
- После успешного `startOrReloadService` интерфейс держит дубликат. Исходный fd можно закрыть сразу после успешного возврата или после `closeService`. До успешного возврата его закрывать нельзя.
- Если `startOrReloadService` бросил исключение после `openTun`, закрыть сохранённый fd. Иначе интерфейс VPN останется поднятым: libbox его не закроет.
- Сокет, переданный в `autoDetectInterfaceControl`, не закрывать. Это fd исходящего соединения, не TUN.

При `FileDescriptor > 0` sing-tun не назначает адреса, маршруты и ip rule. Их ставит приложение через `VpnService.Builder` до `establish()`.

## Protect

`usePlatformAutoDetectInterfaceControl()` возвращает `true`.

Тогда и `AutoDetectInterfaceFunc`, и `ProtectFunc` (`route/network.go`) вызывают `AutoDetectInterfaceControl(fd)`. На Android это `VpnService.protect(fd)`.

- `protect` вернул `false` — метод бросает исключение. Иначе Go считает защиту успешной.
- `usePlatformAutoDetectInterfaceControl() == false` уводит dial на привязку к интерфейсу по умолчанию. Для VPN это интерфейс туннеля, и соединение с сервером уходит в туннель.

Адрес сервера mieru исключается дважды: `route.auto_detect_interface` (protect) и `route_exclude_address` в конфиге TUN. Иначе маршрут `0.0.0.0/0` захватывает сервер.

## Маршруты, которые читает openTun

`BuildAutoRouteRanges(true)` вызывается до `openTun`. `autoRouteUseSubRanges` истинен только на darwin, поэтому на Android `auto_route` даёт `0.0.0.0/0` и `::/0` минус `route_exclude_address`. Поддиапазоны darwin сюда не переносить.

До `establish()` приложение переносит в `VpnService.Builder`:

- `getInet4Address` / `getInet6Address` — адреса интерфейса (`RoutePrefix.address()`, `prefix()`);
- `getMTU()`;
- `getInet4RouteRange` / `getInet6RouteRange` — маршруты, уже с вычетом exclude;
- DNS из `getDNSServerAddress()`, если DNS mode этого требует.

`RoutePrefix`: `address()`, `mask()`, `prefix()`, `string()`. Итератор: `hasNext()`, `next()`. `StringIterator`: `hasNext()`, `len()`, `next()`. `StringBox.getValue()` — для `getDNSMode()`.

DNS в конфиге остаётся udp-detour на outbound mieru, как в `docs/core-contract.md`. Этот каталог форму профиля не меняет.

`getIncludePackage` / `getExcludePackage` на `TunOptions` — пакеты VPN. `OverrideOptions` задаёт те же списки поверх конфига. Пустой итератор и `null` допустимы.

## Что реализовать приложению

`CommandServerHandler` — полный интерфейс gomobile. Все методы обязательны:

```text
int connectSSHAgent() throws Exception;
SystemProxyStatus getSystemProxyStatus() throws Exception;
void serviceReload() throws Exception;
void serviceStop() throws Exception;
void setSystemProxyEnabled(boolean) throws Exception;
void triggerNativeCrash() throws Exception;
void writeDebugMessage(String);
```

Для mieru/TUN достаточно рабочих заглушек: `connectSSHAgent` бросает исключение, `getSystemProxyStatus` возвращает объект с `setAvailable(false)` и `setEnabled(false)`, `serviceStop` вызывает `closeService`, `writeDebugMessage` пишет в logcat. `triggerNativeCrash` не вызывать из UI.

`PlatformInterface` большой. Для этого пути:

| Метод | Поведение |
|---|---|
| `usePlatformAutoDetectInterfaceControl` | `true` |
| `autoDetectInterfaceControl(int)` | `VpnService.protect`; при `false` бросить исключение |
| `openTun(TunOptions)` | собрать Builder, `establish()`, вернуть fd |
| `useProcFS` | `false`. Иначе поиск владельца идёт в `/proc/net` и не вызывает `findConnectionOwner` |
| `localDNSTransport` | `null`, если локальный DNS платформы не нужен. `null` в `baseContext` пропускается |
| `underNetworkExtension` | `false` |
| `includeAllNetworks` | `false`, пока приложению не нужен иной режим VPN |
| `findConnectionOwner` | uid и `setAndroidPackageNames`, либо исключение, если процесс не найден |
| `getInterfaces` | интерфейсы кроме своего TUN; `null`-итератор приведёт к ошибке чтения |
| `startDefaultInterfaceMonitor` / `closeDefaultInterfaceMonitor` | монитор сети. `InterfaceUpdateListener.updateDefaultInterface(name, index, expensive, constrained)` |
| `readWIFIState` | `null`, если состояние не нужно |
| остальные (`sendNotification`, shell, bridge, neighbor, SFTP, SSH, tailscale hostname) | исключение или `null` / `false` |

`useProcFS() == true` читает `/proc/net/tcp` из процесса приложения и не заменяет `ConnectivityManager`/`VpnService`.

## Сборка и проверка, которые уже прошли

`./build-android.sh` из `core`, код 0, 2026-11-11. До этого та же ревизия дважды останавливалась на проверке скрипта, не на компиляции: флаг `-ndk` у `gomobile init` v0.1.13 не существует, а `/usr/bin/strings` на macOS не видит ELF `.rodata`. Итоговый скрипт NDK берёт из `ANDROID_NDK_HOME` и ищет `mieru` через `grep -a`. AAR после этих прогонов — результат `gomobile bind`, не заглушка.

Проверено на архиве: manifest minSdk 26, оба ELF, `javap` шести методов выше, строки `mieru` и `1.14.3` в обоих `.so`.

Не проверено: установка AAR в приложение, `VpnService` на устройстве или эмуляторе, живой сервер mieru, прохождение трафика, смена сетей, снятие интерфейса системой после `closeService`.

## Linux sidecar

Отдельный артефакт, не AAR. `core/build-linux.sh` пишет `core/dist/sing-box-x86_64-unknown-linux-gnu` и `core/dist/linux-REVISION`. Теги те же, `CGO_ENABLED=0`, `-buildvcs=true`. На этой машине `file` показал ELF x86-64, а `sing-box version` и `sing-box check` фикстуры `core/check-loopback.json` прошли в контейнере `ubuntu:24.04` (`--platform linux/amd64`). `check` не вызывает `Start()` и не открывает TUN. macOS-бинарники `build-desktop.sh` этот скрипт не пересобирает.
