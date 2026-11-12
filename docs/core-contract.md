# Контракт ядра — macOS MVP

Дата проверки: 2026-11-11. Хост: macOS 27.2.1, arm64, Go 1.27.3.

Вердикт: закреплённая ревизия `enfein/mbox` пригодна как sidecar macOS. TUN, mieru outbound, проверка конфигурации, исключение соединения с сервером и освобождение маршрутов подтверждены исходниками этой ревизии и локальной сборкой. Поднятие TUN и прохождение трафика через сервер не выполнялись.

Android API/AAR и Windows в этой задаче не исследовались. Они не блокируют macOS-путь. Имена `libbox.BoxService`, собственный Wintun/utun и `tun2proxy` из `PLAN.md` для Desktop не использовать.

## Ревизия

| Поле | Значение |
|---|---|
| Репозиторий | https://github.com/enfein/mbox |
| Ветка-источник | `release-1.14.3` (на момент проверки совпадала с `mieru`) |
| Commit | `1c5b39718ebcd05e6adb31f95246fbf056be9bd7` |
| Коммит | `feat: add mieru protocol`, 2026-10-11 |
| URL | https://github.com/enfein/mbox/commit/1c5b39718ebcd05e6adb31f95246fbf056be9bd7 |
| Модуль | `github.com/sagernet/sing-box` |
| Зависимость протокола | `github.com/enfein/mieru/v3 v3.38.0` |
| Go в `go.mod` | `1.25.5` |
| Исполняемый файл | `./cmd/sing-box`, имя бинарника `sing-box` |

Закреплять commit, а не плавающий `mieru`. README этой ревизии пишет, что ветки `release-*` после создания не меняются, а `mieru` может быть нестабильной. T1 обязана проверить `git rev-parse HEAD`.

Получение:

```sh
git clone --depth 1 --branch release-1.14.3 https://github.com/enfein/mbox.git
git -C mbox rev-parse HEAD
# ожидается 1c5b39718ebcd05e6adb31f95246fbf056be9bd7
```

## Сборка sidecar (проверена)

Полный список `release/DEFAULT_BUILD_TAGS` включает `with_naive_outbound`. На Apple он требует CGO и Xcode. Для mieru и TUN он не нужен. Проверенная сборка — урезанные теги, `CGO_ENABLED=0`.

Из корня исходников mbox:

```sh
export GOTOOLCHAIN=local CGO_ENABLED=0
go build -trimpath \
  -ldflags "-X github.com/sagernet/sing-box/constant.Version=1.14.3 -X runtime.godebugDefault=multipathtcp=0,tlssha1=1 -checklinkname=0 -s -w -buildid=" \
  -tags "with_gvisor,badlinkname,tfogo_checklinkname0" \
  -o sing-box ./cmd/sing-box
```

`-checklinkname=0` и `runtime.godebugDefault=multipathtcp=0,tlssha1=1` взяты из `release/LDFLAGS`. Тег `with_gvisor` нужен для стека TUN `gvisor`. `badlinkname` и `tfogo_checklinkname0` идут вместе с этим флагом линкера.

Фактический результат на этом хосте:

```text
sing-box version 1.14.3
Environment: go1.27.3 darwin/arm64
Tags: with_gvisor,badlinkname,tfogo_checklinkname0
Revision: 1c5b39718ebcd05e6adb31f95246fbf056be9bd7
CGO: disabled
```

Файл: Mach-O 64-bit executable arm64. Сборка завершилась с кодом 0. Каталог сборки был `/tmp/mbox-1.14.3` и в репозиторий не входит. Windows amd64 и Android AAR этой командой не собирались.

## Что принимает ядро

Команды CLI, которые использует приложение:

```text
sing-box check -c <абсолютный-путь> --disable-color
sing-box run   -c <абсолютный-путь> -D <абсолютный-workdir> --disable-color
```

Без `-c` ядро читает `config.json` из текущего каталога. Пароль передаётся только внутри файла конфигурации, не аргументом и не переменной окружения.

`check` создаёт сервис и сразу закрывает его. `Start()` при этом не вызывается, TUN не открывается. Ненулевой `check` запрещает запуск. Успешный `check` не означает, что интерфейс поднят и сервер доступен.

Проверенные конфигурации (`sing-box check --disable-color`):

| Конфигурация | Код | Смысл |
|---|---|---|
| mixed на `127.0.0.1` + mieru TCP + `auto_detect_interface` | 0 | минимальный профиль принимается |
| TUN `gvisor` + mixed loopback + DNS detour + mieru | 0 | каноническая форма ниже принимается |
| то же плюс `route_exclude_address` | 0 | исключение префикса принимается |
| TCP и доменное имя сервера | 0 | `check` домен не отвергает |
| UDP и доменное имя сервера | 0 | `check` это тоже принимает; ограничение ниже проявляется при наборе, не при проверке |
| `transport: QUIC` | 1 | `transport must be TCP or UDP` |
| пустой `password` | 1 | `password is empty` |
| `auto_redirect: true` | 1 | `` `auto_redirect` is only supported on Linux `` |

Ошибка `check` пишется в stderr в виде `FATAL[0000] ...` и даёт код 1.

Транспорт outbound — только строки `TCP` и `UDP`. Обязательны `server`, `username`, `password` и хотя бы одно из `server_port` / `server_ports`. Необязательные поля upstream: `server_ports`, `multiplexing` (`MULTIPLEXING_OFF`, `MULTIPLEXING_LOW`, `MULTIPLEXING_MIDDLE`, `MULTIPLEXING_HIGH`), `traffic_pattern` (стандартный base64 от protobuf `TrafficPattern`).

README ревизии отдельно говорит: UDP outbound не может использовать доменное имя как адрес сервера. В коде это жёстче, чем строка README. mbox не задаёт mieru DNS resolver. Для UDP underlay вызывается `ResolveUDPAddr` с `NilDNSResolver`, поэтому домен на UDP не резолвится в момент набора. `BypassDialerDNS: true` относится только к TCP и отдаёт имя dialer-у sing-box. Контракт это обходит до запуска: приложение само резолвит имя в один IP, пока туннель ещё не поднят, и в ядро пишет уже IP.

Пользовательский UDP через туннель ядро принимает: outbound объявляет сети `tcp` и `udp`, UDP уходит как UDP-over-stream поверх mieru. Это свойство исходника, живым трафиком не проверялось.

## Каноническая конфигурация macOS

Приложение хранит короткий профиль и само собирает JSON ядра. Профиль не является произвольным конфигом sing-box.

Профиль:

```json
{
  "server": "203.0.113.10",
  "server_port": 8964,
  "transport": "TCP",
  "username": "baozi",
  "password": "manlianpenfen",
  "traffic_pattern": "GgQIARAK"
}
```

`traffic_pattern` можно опустить. `transport` — `TCP` или `UDP`. `server` — IP или доменное имя. Порт — одно число 1–65535. Диапазоны `server_ports` и multiplexing в профиль MVP не входят.

Перед `run`, ещё в состоянии disconnected:

1. Если `server` — домен, системный резолвер выбирает один адрес. Предпочесть IPv4. Нет адреса — состояние `error`, процесс не запускать.
2. В outbound записать выбранный IP, не исходное имя.
3. Для IPv4 добавить в TUN `route_exclude_address` значение `<ip>/32`, для IPv6 — `<ip>/128`.
4. Записать файл с правами `0600`. В лог и в аргументы содержимое не попадает.

Пример, который `check` принял с кодом 0. Адрес `203.0.113.10` документации, не боевой сервер. `traffic_pattern` в примере — фиктивная строка из README upstream.

```json
{
  "log": { "level": "info" },
  "dns": {
    "servers": [
      {
        "type": "udp",
        "tag": "remote-dns",
        "server": "1.1.1.1",
        "server_port": 53,
        "detour": "mieru-out"
      }
    ],
    "final": "remote-dns",
    "strategy": "ipv4_only"
  },
  "inbounds": [
    {
      "type": "tun",
      "tag": "tun-in",
      "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
      "auto_route": true,
      "stack": "gvisor",
      "route_exclude_address": ["203.0.113.10/32"]
    },
    {
      "type": "mixed",
      "tag": "mixed-in",
      "listen": "127.0.0.1",
      "listen_port": 12080
    }
  ],
  "outbounds": [
    {
      "type": "mieru",
      "tag": "mieru-out",
      "server": "203.0.113.10",
      "server_port": 8964,
      "transport": "TCP",
      "username": "baozi",
      "password": "manlianpenfen",
      "traffic_pattern": "GgQIARAK"
    }
  ],
  "route": {
    "final": "mieru-out",
    "auto_detect_interface": true
  }
}
```

Поля, которые менять нельзя без новой проверки:

- `stack` остаётся `gvisor`. При включённом теге пустой `stack` означал бы `mixed`; контракт фиксирует `gvisor`.
- `auto_redirect` на macOS не ставить.
- `interface_name` не задавать. Пустое имя на Darwin становится следующим свободным `utunN` (`CalculateInterfaceName`).
- MTU не задавать. Вне Network Extension код подставляет 65535. Живьём не измерялось.
- mixed слушает только `127.0.0.1`. Порт 12080 можно сменить, если он занят; в конфиг пишется фактический.
- DNS-сервер `1.1.1.1` ходит через `detour: mieru-out`. В этой ревизии DNS-транспорт без detour эквивалентен прямому dialer и для туннеля не годится.
- `log.output` не задавать: журнал идёт в stderr. `timestamp` не включать, чтобы строка готовности имела вид `INFO[NNNN] ...`. Уровень `info`.
- `route.final` — `mieru-out`, чтобы трафик TUN и mixed не зависел от отдельного прямого outbound.

`strict_route` в `sing-tun` этой ревизии обрабатывается на Linux и Windows. На macOS он не даёт подтверждённого эффекта, в конфиг его не ставить.

## TUN, права и исключение сервера

На Darwin ядро само создаёт `utun` через `com.apple.net.utun_control`, назначает адрес ioctl `SIOCAIFADDR` / `SIOCAIFADDR_IN6` и в `Start()` добавляет маршруты сырым сокетом `AF_ROUTE` (`RTM_ADD`). При `auto_route` на Darwin это не маршрут `0.0.0.0/0`, а набор префиксов `1/8`, `2/7`, `4/6`, `8/5`, `16/4`, `32/3`, `64/2`, `128/1` и аналогичный набор IPv6, минус `route_exclude_address`. Более узкие локальные маршруты ОС из-за этого не затираются самим фактом наличия широкого префикса; отдельная проверка локальной сети не делалась.

Поднимать этот процесс должен root. У mbox нет macOS-помощника привилегий. Непривилегированный запуск в T0 специально не выполнялся: успешный `Start()` изменил бы таблицу маршрутов машины. Документация TUN говорит, что без привилегий адрес и MTU сами не настраиваются.

Практический путь для T4, без отдельного сетевого стека Rust:

- один экземпляр sidecar;
- старт уже с euid 0 через диалог администратора macOS (`osascript` `do shell script "..." with administrator privileges`);
- команда внутри скрипта — абсолютные `sing-box run -c ... -D ... --disable-color`, stdout и stderr в файл, который читает приложение;
- скрипт печатает PID фонового процесса, приложение его запоминает;
- отказ в диалоге — состояние `error`, процесс не считается запущенным;
- останов — такой же привилегированный `kill -TERM <pid>`, потому что пользовательский процесс не может сигналить root-процессу.

Точная строка AppleScript в T0 не запускалась.

Исключение соединения с сервером — два механизма сразу:

1. `route.auto_detect_interface: true`. На Linux, Windows и macOS это поддерживается. Dialer mieru привязывает сокет к текущему интерфейсу по умолчанию: на Darwin это `IP_BOUND_IF` / `IPV6_BOUND_IF`. Привязка не действует, если у outbound задан свой `bind_interface`; у mieru его не задавать.
2. `route_exclude_address` с префиксом уже выбранного IP сервера, чтобы остальные приложения тоже не отправляли этот адрес в `utun`.

Одного факта `auto_route` недостаточно: документация TUN прямо требует `auto_detect_interface`, `default_interface` или `bind_interface`, иначе исходящие сокеты ядра попадают обратно в туннель. Другие адреса того же домена автоматически не исключаются.

## Готовность, состояния и освобождение

Запуск процесса не есть подключение. Строка журнала `mieru client is started` пишется при создании outbound, до подъёма TUN, и готовностью не считается. У mieru нет отдельного события «сессия с сервером установлена»: набор начинается с первым пользовательским соединением.

Готовность ядра — строка stderr, содержащая `sing-box started (`, при ещё живом процессе. Она пишется после успешного `Box.Start()`, когда inbound, включая TUN, уже прошёл старт. Рядом бывает `inbound/tun[tun-in]: started at utunN`; имя интерфейса брать оттуда, если оно нужно UI. Внутренний монитор стадий ядра — 10 секунд (`constant.StartTimeout`). Приложение ждёт строку готовности 15 секунд. Это таймаут приложения, не флаг CLI.

Останов: `Box.Close()` закрывает область сервиса. Для Darwin `NativeTun.Close()` вызывает `unsetRoutes` (`RTM_DELETE` тех же префиксов) и закрывает fd `utun`, затем сбрасывает кэш DNS (`dscacheutil -flushcache`). До этого доходит обработка `SIGINT` и `SIGTERM`. `SIGHUP` в `run` перечитывает конфиг; приложение его не шлёт. `SIGKILL` этот путь пропускает, маршруты могут остаться. Сначала `SIGTERM`, ждать выход, и только потом `SIGKILL`.

Состояния UI, событие Tauri `vpn-status`:

```json
{
  "state": "disconnected",
  "message": "",
  "since": "2026-11-11T00:00:00Z"
}
```

`state` — одно из `disconnected`, `connecting`, `connected`, `disconnecting`, `error`. `since` — RFC3339 момента входа в состояние. `message` пустой, кроме `error`. В `message` попадает текст stderr без пароля и без `traffic_pattern`.

| Состояние | Когда |
|---|---|
| `disconnected` | sidecar нет |
| `connecting` | привилегии запрошены или процесс есть, строки `sing-box started (` ещё нет |
| `connected` | строка готовности была, процесс жив. Это локальная готовность TUN, не проверка сервера |
| `disconnecting` | отправлен `SIGTERM`, ждём выход |
| `error` | отказ в правах, `check` перед стартом вернул не ноль, процесс умер до готовности, таймаут 15 с, ненулевой выход, обрыв уже после готовности |

Источник ошибки — stderr sidecar и его код выхода. `check` падает с `FATAL[0000]` на stderr и кодом 1. Неожиданный выход после `connected` тоже переводит в `error`; приложение затем пытается убрать процесс тем же привилегированным остановом.

Из `error` команда подключения снова идёт в `connecting`. Команда отключения при живом процессе идёт в `disconnecting`, без процесса — сразу в `disconnected`. Одновременно живёт не больше одного sidecar.

Команды Tauri для T4/T5:

| Команда | Действие |
|---|---|
| `profile_get` | текущий профиль или пусто |
| `profile_set` | проверить поля профиля; пароль в ответе не возвращать |
| `vpn_connect` | собрать конфиг, `sing-box check`, затем привилегированный `run` |
| `vpn_disconnect` | `SIGTERM` управляемого PID |
| `vpn_status_get` | тот же объект, что у события |

Событие и команда статуса называются по полю `state`, не по факту существования процесса.

## Ссылки

mbox эту ревизию URL не принимает: `check` работает только с JSON.

В зависимости `mieru v3.38.0` (`pkg/appctl/url.go`) есть два формата:

- `mieru://` — стандартный base64 protobuf всего `ClientConfig`;
- `mierus://user:pass@host?profile=...&port=...&protocol=TCP&traffic-pattern=...` — одна цель на URL. `protocol` здесь `TCP` или `UDP`. `traffic-pattern` — тот же base64 protobuf, который mbox кладёт в `traffic_pattern`.

Импорт ссылок в команды MVP не входит. `mieru://` шире профиля (много серверов, MTU, handshake, multiplexing). Ссылка с несколькими портами или диапазоном в одно поле `server_port` не отображается. T2 может описать `mierus://` как необязательный источник тех же полей профиля, не как конфиг ядра.

## Распространение

Оба дерева — GPL-3.0-or-later: `LICENSE` mbox и заголовки mieru, плюс текст GPL-3 в `mieru/v3`. У mbox есть дополнительное условие: производная работа не должна использовать имя или намекать на связь с приложением без согласия. Sidecar в версии печатает `sing-box`. Приложение называется своим именем и не представляется как sing-box.

Рядом с бинарником T1 кладёт `LICENSE` mbox, URL и commit этой ревизии. Исходники зависимости mieru доступны как модуль `v3.38.0`. Отдельный юридический вывод этот контракт не делает.

## Что не проверено

- Реальный `run` с TUN от root, появление `utun`, установка и снятие маршрутов, восстановление сети после `SIGTERM` и после `SIGKILL`.
- Доступность сервера mieru, смена внешнего IP, DNS через детур. VPS нет.
- Поведение IPv6, утечки DNS и локальной сети. `strategy: ipv4_only` только ограничивает ответы резолвера; аудита утечек не было.
- Windows, Android AAR, `libbox`, передача TUN fd и socket protect.
- Сборка с полным `DEFAULT_BUILD_TAGS` и с `with_naive_outbound`.

Непроверенный живой туннель не отменяет пригодность ревизии для сборки sidecar и для backend, который управляет этим процессом. Он не доказывает, что VPN уже работает.
