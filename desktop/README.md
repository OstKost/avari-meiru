# Desktop backend

Tauri v2 backend Mieru. Он хранит короткий профиль, собирает конфигурацию ядра и держит одну сессию mbox TUN. Свой TUN/Wintun и `tun2proxy` не используются. Приложение называется Mieru и не представляется как sing-box.

Итоговая Linux-приёмка координатором: [docs/LINUX_SMOKE.md](../docs/LINUX_SMOKE.md). GUI connect/HTTPS, отключение и повторное подключение, скрытие/возврат/выход через D-BusMenu, аварийная очистка и фоновая проверка сервера прошли в Xvfb. Ниже сохранены подробности отдельных прогонов и найденных по пути ошибок; они не заменяют итоговый отчёт. Настоящий desktop/logind-диалог не проверен.

UI находится в `desktop/ui`. Трей — `desktop/src-tauri/src/tray.rs`. Общий код профиля, IPC и сборки конфига — `desktop/src-tauri/crates/mieru-shared`. На Linux привилегированная часть — отдельный headless бинарник `mieru-helper` (`desktop/src-tauri/crates/mieru-helper`), без Tauri и без WebKit.

Платформы: macOS (поведение прежнее) и Linux x86_64. Модули разделены через `cfg`: macOS компилирует `vpn.rs` и supervisor, Linux компилирует `vpn_linux.rs`.

## macOS

GUI не запускается от root. Профиль: `~/Library/Application Support/Mieru/profile.json`, режим `0600`. Sidecar ищется рядом с приложением и в `core/dist` (`sing-box-aarch64-apple-darwin` или `sing-box-x86_64-apple-darwin`).

`vpn_connect` показывает диалог администратора (`osascript` `do shell script … with administrator privileges`) и поднимает тот же бинарник с `--mieru-supervisor`. В shell и AppleScript попадают только абсолютные пути и числовой pid владельца. Поля профиля туда не подставляются. Повышенный процесс вызывает `setsid()`. Запуск без `/usr/bin/nohup`: `/bin/bash -c "( trap '' HUP; exec … </dev/null >>supervisor.log 2>&1 ) & echo $!"`.

Идентификация root-процесса: `proc_pidinfo`, затем `sysctl(KERN_PROC_PID)`, затем `/bin/ps` с `LC_ALL=C` и `TZ=UTC`. Совпадают путь исполняемого файла, время старта и nonce сессии. Nonce либо в аргументах, либо в handshake `state.json`.

Ядро:

```text
sing-box run -c <config.json> -D <workdir> --disable-color
```

Перед `run` выполняется `sing-box check`. Готовность — строка `sing-box started (` в stderr, таймаут 15 секунд. Это локальный старт TUN, не доступность сервера и не смена внешнего IP. На macOS `connected` ставится после этой строки, пока процесс жив.

Останов: файл `stop`, затем `SIGTERM`, до 8 секунд, затем `SIGKILL` только после повторной проверки pid. Падение GUI гасит ядро по pid владельца и heartbeat (GUI пишет его каждые 2 секунды, порог 90 секунд). `SIGKILL` самого supervisor этот путь пропускает.

## Linux

GUI тоже не запускается от root. Каталог данных: `$XDG_DATA_HOME/mieru`, иначе `~/.local/share/mieru`. Профиль `profile.json`, режим `0600`. GUI не ищет sidecar и не принимает путь к нему.

Установленные файлы фиксированы и должны быть обычными файлами root без group/other write и без symlink на пути:

- `/usr/lib/mieru/mieru-helper`
- `/usr/lib/mieru/sing-box`
- политика для упаковки: `/usr/share/polkit-1/actions/app.mieru.policy` (исходник `desktop/src-tauri/linux/app.mieru.policy`, action `app.mieru.desktop.helper`)

В политике только аннотация `org.freedesktop.policykit.exec.path`. `org.freedesktop.policykit.exec.allow_gui` нет: helper headless и не нуждается в `DISPLAY` или `XAUTHORITY`. Упаковка эту политику не ставит в этой задаче: файлы в `/usr/lib/mieru` кладёт пакет. Helper линкуется с libc, не с WebKit и не с GTK.

Подключение: `/usr/bin/pkexec /usr/lib/mieru/mieru-helper --session <32 hex> --owner-pid <pid GUI>`. Других аргументов нет. Пароль и `traffic_pattern` в argv не передаются. Сокет выводится из uid и session: `/run/user/<uid>/mieru/<session>/helper.sock`, владелец — пользователь GUI, режим `0600`. Кадры IPC — длина u32 BE и JSON, не больше 64 КиБ. Helper сверяет клиента через `SO_PEERCRED` (uid и pid GUI). GUI на том же сокете сверяет сервер: uid 0 и pid равен pid из Hello. Секреты в журнал и в текст ошибок не пишутся.

GUI принимает helper только при uid 0, совпавшем pid и starttime, и только после `SO_PEERCRED` на подключённом сокете. JSON Hello сам по себе не достаточен. Обычный путь: `/proc/pid/exe` равен `/usr/lib/mieru/mieru-helper`. Если имя exe — `rosetta`, `rosetta-wrapper` или начинается с `qemu-`, argv обязан быть одной из двух форм. Нативная: argv0 точно `/usr/lib/mieru/mieru-helper`. Интерпретатор: argv0 — путь с таким именем файла, а позже отдельный аргумент точно `/usr/lib/mieru/mieru-helper`. В обеих формах `--session` и `--owner-pid` — отдельные аргументы, и следующий аргумент каждого флага — точные session и pid текущего GUI. Если desktop-пользователь не может прочитать exe (EACCES у root helper и на нативном Linux, и под Rosetta), принимается та же форма argv, и только когда установленный `/usr/lib/mieru/mieru-helper` проходит проверку root-owned executable. Подстрока, склеенный `--session=<nonce>`, nonce не сразу после флага, чужой pid сокета и отключение проверки не подходят. Установленные файлы по-прежнему проверяются как root-owned tree.

Одна сессия: `flock` в каталоге данных GUI и `flock` на `/run/mieru/instance.lock`. Helper сверяет владельца по uid, gid и starttime из `/proc` и по heartbeat 90 секунд. Обрыв сокета или смерть владельца останавливает ядро. Перед `SIGTERM` и последующим `SIGKILL` helper подтверждает pid тем же правилом: обычный `/proc/pid/exe` равен `/usr/lib/mieru/sing-box`, а argv содержит путь конфига этой сессии отдельным аргументом. Интерпретатор Rosetta или qemu-user принимается только когда и путь sidecar, и путь конфига — целые аргументы. Конфиг helper собирает сам из уже проверенного профиля и кладёт его в `/run/mieru/<session>` с режимом `0600`. После готовности файл удаляется. Единственный exec — `/usr/lib/mieru/sing-box` с аргументами `run -c <наш конфиг> -D <workdir> --disable-color`.

`connected` на Linux ставится только после строки `sing-box started (` и короткой проверки через туннель: DNS A `example.com` на `172.19.0.2:53`, затем TCP на порт 443 выбранного адреса. Это не HTTP 200 и не сверка внешнего IP.

Ошибки прав различаются и не содержат сырой stderr:

- отказ или отмена диалога — `administrator privileges were not granted`
- нет polkit-агента — `no polkit authentication agent is available`
- нет установленного helper — `the installed VPN helper is missing`

Отмена во время диалога посылает `SIGTERM` процессу `pkexec`, который запустил GUI. После старта helper GUI шлёт IPC `Disconnect` и ждёт выхода. GUI не шлёт `SIGKILL` root-helper: такой сигнал пропускает снятие маршрутов.

DNS как в `core/LINUX_SMOKE.md`: у TUN `dns_mode: hijack`, правило маршрута `protocol: dns` / `action: hijack-dns`, DNS `1.1.1.1:53` с `detour: mieru-out` и `strategy: ipv4_only`. Адрес `172.19.0.2` — следующий адрес сети `172.19.0.1/30`, его перехватывает gVisor. Helper подменяет `/etc/resolv.conf` на `nameserver 172.19.0.2` и `options timeout:2 attempts:1`, а при выходе восстанавливает файл или прежнюю ссылку. Если запись замены не удалась, helper пытается вернуть исходное содержимое. `resolvectl` вызывается, только если `/usr/bin/resolvectl` есть; его ошибка сессию не роняет, если `resolv.conf` уже переписан. Если `resolvectl dns` применился, а `resolvectl domain` нет, helper сразу делает `resolvectl revert` и не запоминает интерфейс. `auto_redirect` и `strict_route` не задаются. Подмена `resolv.conf` начинается только после строки готовности ядра: ошибка `check`, запуска или ожидания готовности DNS не трогает. Ошибка probe, обрыв IPC и смерть владельца попадают в `stop`, который снимает ядро и возвращает DNS.

IPv6-адреса TUN те же, что в smoke (`fdfe:dcba:9876::1/126`). `auto_route` уводит IPv6 в tun, отдельного bypass нет. Живой IPv6 egress и UDP эта сборка не доказывала.

## Окно и трей

Если трей создался, закрытие окна только прячет его: VPN остаётся. Если трея нет, закрытие вызывает останов VPN и выход из приложения. Ошибка трея не мешает запуску. Пункт «Выход» в трее сначала останавливает ядро, затем завершает приложение. На macOS с живым треем окно по-прежнему прячется.

## Команды для UI

Аргументов нет, кроме `profile_set`. Ошибки — строки без пароля и без `traffic_pattern`.

| Команда | Аргумент | Ответ |
|---|---|---|
| `profile_get` | нет | профиль или `null` |
| `profile_set` | `{ "profile": { ... } }` | тот же профиль без пароля |
| `vpn_connect` | нет | объект статуса |
| `vpn_disconnect` | нет | объект статуса |
| `vpn_status_get` | нет | объект статуса |
| `vpn_log_get` | нет | `{ "lines": ["..."] }` |
| `server_health_check` | нет | проверка сервера через loopback mixed, без TUN |

Событие `vpn-status` несёт тот же объект, что `vpn_status_get`:

```json
{ "state": "disconnected", "message": "", "since": "2026-11-11T00:00:00Z" }
```

`state`: `disconnected`, `connecting`, `connected`, `disconnecting`, `error`. `since` — RFC3339 UTC. `message` пустой, кроме `error`.

`profile_get` и ответ `profile_set` не содержат `password`. Сохранённый пароль остаётся в файле профиля. Если UI получило профиль без пароля, при следующем сохранении оно должно передать пароль снова.

`vpn_log_get` возвращает до 200 последних непустых строк журнала. Пароль и `traffic_pattern` заменены на `[redacted]`.

## Профиль и конфигурация ядра

Перед `run` backend резолвит домен в один адрес и предпочитает IPv4. В outbound попадает этот IP. Для IPv4 в TUN пишется `route_exclude_address` вида `<ip>/32`, для IPv6 — `<ip>/128`. Плюс `route.auto_detect_interface: true`. Другие адреса того же домена не исключаются.

Общая конфигурация: TUN `gvisor`, адреса `172.19.0.1/30` и `fdfe:dcba:9876::1/126`, mixed только на `127.0.0.1` (порт 12080 или следующий свободный до 12179), DNS `1.1.1.1` через `detour: mieru-out`, `strategy: ipv4_only`, `route.final: mieru-out`. Не задаются `auto_redirect`, `interface_name`, MTU, `strict_route`, `bind_interface`, `log.output` и `log.timestamp`. На Linux к этому документу добавляются только `dns_mode: hijack` и правило `hijack-dns`. Успешный `sing-box check` не означает, что туннель поднят.

## Проверено

Хост: macOS arm64, Docker Desktop, 2026-11-12. Локальной Linux VM нет. Старый VPS не использовался. Юнит-тесты предыдущей сборки не повторялись.

Linux x86_64, `--platform linux/amd64`, Rust 1.83.0. Кэш — тома `mieru-al3-cargo-home` и `mieru-al3-target`. `cargo test -p mieru-helper --release sidecar_identity_accepts` — 1 passed. `cargo build -p mieru-helper --release` — код 0, 3.54 с. Обёртка сборки `BUILD_WRAPPER 0`.

Runtime: Ubuntu 24.04 amd64, сеть `mieru-al3-net`, `--cap-add NET_ADMIN --cap-add SYS_ADMIN --device /dev/net/tun`, без `--network=host`. Установлены `/usr/lib/mieru/mieru-helper` и `/usr/lib/mieru/sing-box` от root, режим `0755`. Политика из репозитория без `allow_gui`. Пользователь IPC — обычный uid, не root. Docker не даёт снять bind `/etc/resolv.conf` (`umount` оставляет точку монтирования). Поверх `/etc` смонтирована отдельная копия, и видимый `resolv.conf` — обычный файл; production по-прежнему делает `unlink`, EBUSY не обрабатывается. Локальный mita 3.38.0 (пакет сверен с sha256 `5170555aace38f76862541587b632d61848bba12f2e845b057977c9f9257e59a`) слушал TCP 28443 в этой же сети. В контейнере нет systemd, поэтому сервер запущен как `mita run` с `MITA_CONFIG_JSON_FILE`. `mita status`: `mita server status is "RUNNING"`. Учётные данные фиктивные.

Обёртка runtime `RUNTIME_WRAPPER 0`, в логе `PHASE runtime-ok`:

- `pkexec` без агента: код 127, класс `no polkit authentication agent is available`. Helper не запущен.
- `pkexec` при правиле polkit `Result.NO` на `app.mieru.desktop.helper`: код 127, класс `administrator privileges were not granted`. Helper не запущен. Это не живой диалог.
- Успешный путь — прямой запуск установленного helper от root, то есть процесс после аутентификации. Сервер `203.0.113.10`: статус `error tunnel check failed`, затем `CLEAN` и восстановлен обычный файл `resolv.conf`.
- Connect и disconnect к локальному mita: статус `connected`, `ip route get 1.1.1.1` дал `via 172.19.0.2 dev tun0 table 2022`. После disconnect `CLEAN` и тот же обычный файл DNS.
- Смерть владельца при symlink `/etc/resolv.conf` → `/etc/resolv.stub`: тот же маршрут, `curl -4 --max-time 25 https://example.com` вернул 200, затем `CLEAN` и ссылка восстановлена. Stub не изменился.
- Закрытие IPC при живом владельце: тот же маршрут, `CLEAN`, обычный файл DNS восстановлен.

Повторно используемые пути: том `mieru-al3-target`, файл `/cargo-target/release/mieru-helper`; том `mieru-al3-cargo-home` — кэш cargo. Образ `mieru-al3-runtime-base` — только кэш apt этого прогона.

GUI smoke, тот же хост, 2026-11-12. Образ `mieru-gui-xvfb` (Ubuntu 24.04 amd64), сеть `mieru-al3-net`, `--cap-add NET_ADMIN --cap-add SYS_ADMIN --device /dev/net/tun`, без `--network=host`. Файлы взяты из `packaging/linux/dist/build/out` и положены в `/usr/bin/mieru`, `/usr/lib/mieru/mieru-helper` и `/usr/lib/mieru/sing-box`. GUI заново не собирался. `diff` политики пакета и `desktop/src-tauri/linux/app.mieru.policy` пустой, `allow_gui` нет.

Окно под Xvfb `:99 -ac` от обычного пользователя: `xwininfo` id `0x600003`, заголовок `Avari Mieru`, 420×720. Снимок корня после первой попытки подключения показывает меню «Правка / Вид / Профиль / Окно» и экран «Ошибка соединения» с локальным mita `172.23.0.2:28443 TCP`. WebKit в контейнере запущен с `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1`. Это не рабочий стол.

Профиль записан со страницы. Поля server, port, username и password приняты. Файл `~/.local/share/mieru/profile.json`, режим `0600`, username `runtime-user`, порт 28443, транспорт TCP. Пароль в лог не попал.

Агент `/usr/lib/policykit-1-gnome/polkit-gnome-authentication-agent-1` после session bus пишет `Unable to determine the session we are in: No session for pid`. Окна authenticate нет. UI: `no polkit authentication agent is available`. Это не диалог администратора.

Правило polkit `Result.YES` только внутри контейнера (`disposable-container-only not-a-real-admin-dialog`) пускает pkexec дальше: появляется `/run/user/<uid>/mieru/<session>`, `tun0` нет. UI: `could not confirm the elevated process`. Процесс GUI здесь — `/run/rosetta/rosetta /usr/bin/mieru`. Тот ELF сравнивал exe строго с `/usr/lib/mieru/mieru-helper`. Обёртка `SMOKE_WRAPPER 1`, в логе `TUN_TIMEOUT connect`. HTTPS 200, снятие tun, закрытие окна и смерть владельца на этом снимке не показаны. Повторный ELF с проверкой имени exe всё ещё отвечает `could not confirm the elevated process`: desktop-пользователь получает EACCES на `/proc/pid/exe` root helper, при этом argv содержит целый `/run/rosetta/rosetta` и точный путь helper. Исходник после этого прогона принимает непрочитанный exe и на нативном Linux: argv0 точно `/usr/lib/mieru/mieru-helper` либо имя файла argv0 — интерпретатор, дальше отдельный путь helper, пары `--session` и `--owner-pid`, установленный root-owned executable, и `SO_PEERCRED` сокета с uid 0 и pid, равным Hello.pid.

Снимок build-only `mieru` `b824cac0a7f7c70671ae8d1996853672c55616e083f4a638235fede01ed2d1e4` и `mieru-helper` `61041715953adf43191d6c86ecfaeb43c163deaaa59c5b19d1363856471b469d` на Xvfb доходит до туннеля. UI: кнопка `Отключить`, статус `Подключено`. Root видит exe `rosetta`, argv с точным путём helper, session и pid GUI, uid 0. На время туннеля `/etc/resolv.conf` содержит `nameserver 172.19.0.2`. `curl -4 https://example.com` вернул 200. Пароль в проверенные журналы не попал (`LEAK_OK`). Это не диалог logind и не рабочий стол.

Отключение того же снимка пишет в UI `could not stop the managed core`, хотя сразу после этого `tun0` нет, таблица 2022 пуста, процесс helper отсутствует, DNS совпадает со снимком до подключения. Следующий connect получает `a managed core is already running`. Скрытие окна, показ и смерть владельца на этом прогоне не выполнялись. pkexec подменяет себя helper-ом, поэтому после выхода helper остаётся зомби, пока GUI не сделает wait. `/proc/pid/stat` зомби сохраняет starttime, и прежняя проверка считала его живым все 20 секунд. Исходник после прогона считает состояние `Z` и `X` неживым и дополнительно дожидается собственного child. Текущее дерево `out` эту правку не содержит. Пакеты не ставились.

Ранний panic `Failed to setup app: Permission denied (os error 13)` дал тестовый `install -d`: родители `~/.local` остались у root. После записи `~/.local`, `~/.cache` и `~/.config` тому же пользователю тот же ELF открывает окно. Код приложения из-за этого не менялся.

## Не проверено

Живой диалог polkit в сеансе logind, реальный рабочий стол, IPv6 egress, UDP и смена внешнего IP графическим клиентом. `/usr/bin/resolvectl` в тестовом образе не было, поэтому ветка частичного `resolvectl revert` на runtime не исполнялась. `sing-box check` и unit-тесты туннель не поднимают. GUI-тест ставил дерево `out/`; установка и удаление пакетов проверены отдельно. Windows не проверялась. Xvfb и правило YES не заменяют рабочий стол и диалог администратора. Ошибки UI-отключения/повторного подключения и фонового пробника из ранних прогонов исправлены и перепроверены координатором в итоговом отчёте.
