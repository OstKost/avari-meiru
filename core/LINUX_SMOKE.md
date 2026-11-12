# Linux smoke — sidecar x86_64

Проверка: 2026-11-11, примерно 23:12–23:16 МСК (20:12–20:16 UTC). Срок задачи был 23:25 МСК. GUI Linux не проверялся. macOS AWG и host network не использовались.

Вердикт: sidecar `core/dist/sing-box-x86_64-unknown-linux-gnu` в контейнере `ubuntu:24.04 --platform linux/amd64` провёл реальный TCP. Прямой выход контейнера был `109.107.176.224`. И loopback-прокси, и TUN дали HTTPS 200 и внешний адрес `31.58.213.192`. После `SIGTERM` правила и маршруты контейнера вернулись к снимку до старта, интерфейс `tun0` исчез.

## Среда

| Поле | Значение |
|---|---|
| Docker | Desktop 29.9.3, демон aarch64 |
| Образ | `ubuntu:24.04`, уже был локально |
| Платформа | `--platform linux/amd64`, `uname -m` напечатал `x86_64` |
| Сеть | bridge Docker, не `--network host` |
| TUN | `--cap-add NET_ADMIN` и `--device /dev/net/tun`. Узел оказался символьным устройством |
| Бинарник | `core/dist/sing-box-x86_64-unknown-linux-gnu` |
| Версия | sing-box 1.14.3, `go1.27.3 linux/amd64`, теги `with_gvisor,badlinkname,tfogo_checklinkname0`, revision `1c5b39718ebcd05e6adb31f95246fbf056be9bd7`, CGO disabled |

В образе нет `curl` и нет пакета CA. Запросы делал статический curl linux/x86_64 с отдельным CA-bundle. В репозиторий они не входят. Конфиг собран во временном каталоге вне репозитория из `~/Library/Application Support/Mieru/vps-test-profile.json` и удалён. В argv и в этот файл пароль, имя и полный JSON не записывались.

Профиль: `transport=TCP`, порт `28443`, сервер — IPv4, тот же адрес, что и ожидаемый выход VPS. В outbound записан этот IP. Для TUN он исключён как `/32`. `auto_detect_interface` включён. `stack` — `gvisor`. `auto_redirect` не задавался.

## Loopback proxy

Контейнер без `NET_ADMIN`. Inbound mixed `127.0.0.1:12080`, outbound mieru, `route.final=mieru-out`.

До запуска sidecar прямой `https://api.ipify.org` вернул HTTP 200 и `109.107.176.224`.

После строки `sing-box started (0.04s)`:

| Запрос | Результат |
|---|---|
| `https://api.ipify.org` через `http://127.0.0.1:12080` | HTTP 200, тело `31.58.213.192` |
| `https://example.com` через тот же прокси | HTTP 200 |

Процесс остановлен `SIGTERM`, без `SIGKILL`. Код `docker run`: 0.

## TUN

Тот же образ и bridge. Добавлены только `NET_ADMIN` и `/dev/net/tun`. Конфиг: tun `172.19.0.1/30` и `fdfe:dcba:9876::1/126`, `auto_route`, `stack=gvisor`, `route_exclude_address` сервера `/32`, DNS udp `1.1.1.1` с `detour=mieru-out`. Запросы curl шли с `-4` и `--resolve`, без переменных прокси.

`sing-box started (0.06s)`, журнал: `inbound/tun[tun-in]: started at tun0`. Появился интерфейс `tun0`. В журнале есть входящие соединения с `172.19.0.1` на `104.26.12.205:443` и `172.66.147.243:443`.

| Запрос | Результат |
|---|---|
| `https://api.ipify.org` без прокси | HTTP 200, тело `31.58.213.192` |
| `https://example.com` без прокси | HTTP 200 |

Код `docker run`: 0. Останов `SIGTERM`, `TUN_SIGKILL=0`.

## Маршруты

Пока sidecar жив, основная таблица сохраняет `default via 172.17.0.1 dev eth0`. Трафик уходит правилами:

- `9000` — `172.19.0.0/30` lookup `2022`
- `9001` — lookup `2022` с `suppress_prefixlength 0`
- `9002` — lookup `main` с `suppress_prefixlength 0`, плюс `iif tun0` lookup unspec
- `9003` — lookup `2022` для `iif lo`
- `9010` — broadcast unspec

Таблица `2022` содержит набор префиксов через `172.19.0.2 dev tun0`: это `0.0.0.0/0` минус исключённый `/32` сервера, а не один маршрут `0.0.0.0/0`. Рядом IPv6 default таблицы `2022` через `fdfe:dcba:9876::2 dev tun0`. В main добавлен только link-route `172.19.0.0/30 dev tun0`.

После `SIGTERM` и короткой паузы:

- `ip rule` снова только `local`, `main`, `default`;
- `ip route show table all` совпал со снимком до старта: default через `eth0`, префикс docker `172.17.0.0/16`, локальные адреса `lo` и `eth0`;
- `tun0`, `172.19.0.0/30` и адреса `fdfe:dcba:9876::/126` отсутствуют.

Отдельное побайтовое сравнение `/proc/net/route` сразу после первого TUN-прогона не совпало, хотя `tun0` в `/sys/class/net` уже не было. Следующий прогон с `diff` после паузы дал пустой diff main-таблицы. Итоговым свидетельством восстановления считается вывод `ip rule` и `ip route show table all`. Побайтовый `/proc/net/fib_trie` после одного прогона не совпал; содержимое правил и таблиц `ip` при этом совпало.

## Команды

Все `docker run` с `--rm` и `--platform linux/amd64`. Коды:

| Команда | Код |
|---|---|
| `docker run ubuntu:24.04 uname -m` | 0, вывод `x86_64` |
| проба `/dev/net/tun` и CA | 0, устройство char, CA в образе нет |
| контейнер loopback proxy | 0 |
| контейнер TUN с curl | 0 |
| контейнер `diff` `/proc/net/route` | 0, diff после останова пустой |
| контейнер `ip rule` / `ip route show table all` | 0 |
| контейнер DNS hijack, `nslookup` и HTTPS по имени | 0 |

`sing-box check` в этой задаче не был доказательством. `sing-box run` выполнялся только внутри контейнера.

## Не проверено

- Графический клиент Linux и рабочий стол.
- Сеть macOS, интерфейсы AWG, маршруты хоста.
- UDP-транспорт mieru. Профиль был TCP.
- Повторная смена сети, обрыв VPS и долгая сессия.
- IPv6-выход наружу. Маршруты IPv6 в таблице `2022` поднимались и снимались, отдельного HTTPS по IPv6 не было.
- IPv6 DNS. В `resolv.conf` контейнера был только IPv4 `172.19.0.2`, curl шёл с `-4`.

Предыдущий TUN-прогон с `--resolve` DNS не доказывал. Это закрыто отдельным прогоном ниже.

## DNS через mieru

Проверка около 23:22 МСК, один прогон, без повторов IP/маршрутов. Контейнер тот же: `ubuntu:24.04 --platform linux/amd64`, bridge, `NET_ADMIN`, `/dev/net/tun`. Хостовый `resolv.conf` и macOS не менялись.

Docker по умолчанию резолвит через `127.0.0.11`. В этой ревизии DNS на адреса собственных интерфейсов хоста, включая loopback, не перехватывается. Поэтому внутри контейнера, уже после `sing-box started`, `/etc/resolv.conf` заменён на:

```text
nameserver 172.19.0.2
options timeout:2 attempts:1
```

`172.19.0.2` — не отдельный резолвер. При `address: 172.19.0.1/30` и пустом `dns_address` sing-box берёт следующий адрес префикса. С `auto_route` режим DNS по умолчанию `hijack`; в прогоне `dns_mode` задан явно. UDP на `172.19.0.2` стек gvisor помечает как hijack-dns и не выпускает наружу Docker.

Конфиг, который нужен backend, поверх уже принятого TUN:

```json
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
    "dns_mode": "hijack"
  }
],
"route": {
  "rules": [
    { "protocol": "dns", "action": "hijack-dns" }
  ]
}
```

Остальное без изменений: `auto_route`, `stack=gvisor`, `route_exclude_address` сервера `/32`, `auto_detect_interface`, `final=mieru-out`. Пароль и полный JSON в репозиторий не входят.

Доказательство, без `--resolve` и без прокси:

| Шаг | Факт |
|---|---|
| `nslookup api.ipify.org 172.19.0.2` | код 0. Ответ: `104.26.13.205`, `172.67.74.152`, `104.26.12.205` |
| журнал | `inbound DNS packet from 172.19.0.1:35115` |
| журнал | `outbound/mieru[mieru-out]: outbound UoT packet connection to 1.1.1.1:53` |
| `https://api.ipify.org` обычным именем | HTTP 200, тело `31.58.213.192` |
| журнал HTTPS | `outbound connection to 104.26.13.205:443` — адрес из ответа DNS, не из `--resolve` |

`UoT` здесь — UDP DNS, унесённый через mieru TCP. Запрос к `1.1.1.1:53` сделал сам sidecar через `detour`, не резолвер Docker. Останов `SIGTERM`, `TUN_SIGKILL=0`. Код `docker run`: 0.

Первая попытка этого прогона не отправила DNS: в контейнер смонтировались пустые каталоги вместо curl и busybox. Запроса к VPS тогда не было. Вторая попытка — та, что в таблице.

## Очистка

Контейнеры `mieru-smoke-proxy`, `mieru-smoke-tun`, `mieru-smoke-rules` и `mieru-dns` запускались с `--rm`. Временные JSON с паролем удалены и в `core/` не копировались.
