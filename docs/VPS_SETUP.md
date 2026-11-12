# Руководство по развертыванию сервера Mieru (mita) на VPS

## 1. Исходные условия и параметры окружения

### 1.1. Параметры тестового VPS
- **Хост**: `31.58.213.192`
- **SSH доступ**: `ssh -o BatchMode=yes -o ConnectTimeout=10 root@31.58.213.192` (сохраняется StrictHostKeyChecking).
- **Срок жизни**: временный сервер (несколько часов).

### 1.2. Результаты Read-Only Preflight аудита
- **ОС**: Ubuntu 24.04.4 LTS (Noble Numbat), Linux 6.8.0-111-generic
- **Архитектура**: `x86_64` (`amd64`)
- **Сетевой интерфейс**: `ens18` (`31.58.213.192/24`), исходящий публичный IP: `31.58.213.192` (совпадает с адресом подключения)
- **Службы**: `systemd`, `ssh.service` (OpenSSH 9.6p1), `systemd-resolved`
- **Занятые порты**:
  - `tcp 0.0.0.0:22`, `tcp [::]:22` (`sshd`)
  - `tcp 127.0.0.53:53`, `tcp 127.0.0.54:53`, `udp 127.0.0.53:53`, `udp 127.0.0.54:53` (`systemd-resolve`)
  - Все непривилегированные порты (1024–65535) свободны.
- **Firewall**:
  - UFW: `inactive`
  - iptables / ip6tables: политики по умолчанию `ACCEPT`, активных правил фильтрации нет.
  - nftables: ruleset пуст.

---

## 2. Закрепленные версии и источники

- **Клиентское ядро проекта**: `enfein/mbox` на ревизии `1c5b39718ebcd05e6adb31f95246fbf056be9bd7` (ветка `release-1.14.3`), зависимость: `github.com/enfein/mieru/v3 v3.38.0`.
- **Серверный компонент**: `mita` v3.38.0 (официальный релиз проекта `enfein/mieru`).
- **Пакет для Ubuntu x86_64**: `mita_3.38.0_amd64.deb`
- **Источник**: [GitHub Releases enfein/mieru v3.38.0](https://github.com/enfein/mieru/releases/tag/v3.38.0)
- **URL загрузки**: `https://github.com/enfein/mieru/releases/download/v3.38.0/mita_3.38.0_amd64.deb`
- **Контрольная сумма SHA-256**:
  ```text
  5170555aace38f76862541587b632d61848bba12f2e845b057977c9f9257e59a  mita_3.38.0_amd64.deb
  ```
  *(Сверено с файлом `mita_3.38.0_amd64.deb.sha256.txt` из официального релиза).*

---

## 3. Последовательность команд установки и настройки

> [!NOTE]
> Все команды выполняются на удаленном VPS под пользователем `root`. Вместо реальных учетных данных используются плейсхолдеры `<USERNAME>`, `<PASSWORD>`, `<PORT>` (выбран свободный порт `28443`).

### 3.1. Загрузка и проверка пакета
```bash
mkdir -p /tmp/mita-install && cd /tmp/mita-install

curl -LSO https://github.com/enfein/mieru/releases/download/v3.38.0/mita_3.38.0_amd64.deb
echo "5170555aace38f76862541587b632d61848bba12f2e845b057977c9f9257e59a  mita_3.38.0_amd64.deb" | sha256sum -c
```
*Ожидаемый вывод:* `mita_3.38.0_amd64.deb: OK`

### 3.2. Установка deb-пакета
```bash
dpkg -i mita_3.38.0_amd64.deb
```
Пакет выполняет следующие действия:
1. Создает системного пользователя и группу `mita`.
2. Создает рабочие каталоги `/etc/mita`, `/var/lib/mita`, `/var/run/mita` с правами доступа для пользователя `mita`.
3. Устанавливает бинарный файл `/usr/bin/mita`.
4. Регистрирует и запускает службу `mita.service`.

### 3.3. Создание конфигурации сервера
Создается файл `/etc/mita/server_config.json` с ограниченными правами (`0600`, владелец `mita:mita`):

```json
{
  "portBindings": [
    {
      "port": 28443,
      "protocol": "TCP"
    }
  ],
  "users": [
    {
      "name": "<USERNAME>",
      "password": "<PASSWORD>"
    }
  ],
  "loggingLevel": "INFO",
  "mtu": 1400,
  "dns": {
    "dualStack": "PREFER_IPv4"
  }
}
```

Команды фиксации прав и применения конфигурации:
```bash
chmod 600 /etc/mita/server_config.json
chown mita:mita /etc/mita/server_config.json

# Применение конфигурации через официальный CLI mita
mita replace config /etc/mita/server_config.json

# Запуск проксирования
mita start
```

### 3.4. Проверка состояния службы и слушающего сокета
```bash
# Проверка внутреннего состояния mita
mita status
# Ожидаемый вывод: mita server status is "RUNNING"

# Проверка слушающего сокета TCP
ss -tulpn | grep 28443
# Ожидаемый вывод: tcp LISTEN ... *:28443 users:(("mita",pid=...,fd=...))

# Проверка systemd службы
systemctl status mita --no-pager
```

---

## 4. Конфигурация Firewall (минимальный diff)

- **Исходное состояние VPS**: UFW не включен, iptables имеет политики по умолчанию `ACCEPT` без фильтрующих правил. Порт `28443/tcp` открыт и доступен.
- **Минимальный diff при использовании UFW / iptables в защищенных окружениях**:
  ```bash
  # Для UFW:
  ufw allow 28443/tcp comment "Mieru mita TCP proxy"

  # Для iptables:
  iptables -A INPUT -p tcp --dport 28443 -m comment --comment "Mieru mita TCP" -j ACCEPT
  ```
- SSH порт `22/tcp` не изменялся и не закрывался. Перезагрузка сервера и сброс сетевых правил не производились.

---

## 5. Клиентский профиль (macOS)

Клиентский профиль сформирован в формате, строго соответствующем `configs/profile.schema.json`.

### 5.1. Безопасное хранение вне репозитория
- **Путь**: `~/Library/Application Support/Mieru/vps-test-profile.json`
- **Права доступа**: `0600` (`-rw-------`), доступен только текущему пользователю macOS.
- **Существующий файл** `profile.json` не перезаписывался.

### 5.2. Структура профиля (с плейсхолдерами)
```json
{
  "server": "31.58.213.192",
  "server_port": 28443,
  "transport": "TCP",
  "username": "<USERNAME>",
  "password": "<PASSWORD>"
}
```

---

## 6. Проверка проксирования с Mac (без root TUN)

Тестирование выполнено с локального Mac без root-привилегий, без вызова системного TUN и без изменения таблиц маршрутизации macOS.

### 6.1. Тестовый клиент sing-box
Использован собранный бинарник проекта:
`core/dist/sing-box-aarch64-apple-darwin` (версия sing-box 1.14.3 с поддержкой протокола Mieru).

### 6.2. Временная конфигурация тестового клиента
Временный конфигурационный файл (inbound `mixed` на loopback):
```json
{
  "log": { "level": "info" },
  "inbounds": [
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
      "server": "31.58.213.192",
      "server_port": 28443,
      "transport": "TCP",
      "username": "<USERNAME>",
      "password": "<PASSWORD>"
    }
  ],
  "route": {
    "final": "mieru-out"
  }
}
```

### 6.3. Как повторить локальную проверку

Из корня проекта создайте приватный временный каталог и сохраните JSON из раздела 6.2 в его `config.json`, заменив плейсхолдеры данными своей учётной записи. Не вставляйте пароль в аргументы команд. Перед запуском убедитесь, что порт `12080` свободен и основной VPN-клиент Mieru отключён.

```bash
umask 077
MIERU_SMOKE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mieru-vps-smoke.XXXXXX")"
# Сохраните JSON из раздела 6.2 в "$MIERU_SMOKE_DIR/config.json".
chmod 600 "$MIERU_SMOKE_DIR/config.json"
./core/dist/sing-box-aarch64-apple-darwin check -c "$MIERU_SMOKE_DIR/config.json" --disable-color
./core/dist/sing-box-aarch64-apple-darwin run -c "$MIERU_SMOKE_DIR/config.json" --disable-color >"$MIERU_SMOKE_DIR/core.log" 2>&1 &
MIERU_SMOKE_PID=$!
# Дождитесь строки sing-box started в приватном core.log.
curl -4 --fail --max-time 10 -x socks5h://127.0.0.1:12080 https://icanhazip.com
curl -4 --fail --max-time 10 -x http://127.0.0.1:12080 https://ifconfig.me
# Остановите именно клиент, запущенный выше в этой shell-сессии.
kill -TERM "$MIERU_SMOKE_PID"
wait "$MIERU_SMOKE_PID"
rm -rf "$MIERU_SMOKE_DIR"
```

Ожидаемый IP обоих запросов — `31.58.213.192`, пока VPS существует. На Intel используйте бинарник `sing-box-x86_64-apple-darwin`. Эта конфигурация не содержит TUN и не меняет системные маршруты. При ошибке запуска/запроса остановите этот же тестовый процесс и удалите временный каталог; не продолжайте проверку с чужим процессом, занявшим порт.

### 6.4. Результаты выполненной проверки
1. **Валидация конфигурации**:
   ```bash
   ./core/dist/sing-box-aarch64-apple-darwin check -c /tmp/test-sing-box-config.json --disable-color
   # Результат: код 0 (PASSED)
   ```
2. **Проверка через SOCKS5**:
   ```bash
   curl -s -4 --max-time 10 -x socks5h://127.0.0.1:12080 https://icanhazip.com
   # Результат: 31.58.213.192 (код 0)
   ```
3. **Проверка через HTTP прокси**:
   ```bash
   curl -s -4 --max-time 10 -x http://127.0.0.1:12080 https://ifconfig.me
   # Результат: 31.58.213.192 (код 0)
   ```
4. **Остановка клиента**:
   Процессу sing-box отправлен сигнал `SIGTERM`. Процесс корректно завершился, сокет `127.0.0.1:12080` освобожден, временный файл удален.
5. **Метрики сервера на VPS**:
   Команда `mita get metrics` зафиксировала успешное прохождение трафика (исходящий и входящий объем данных зафиксирован по созданному пользователю без единой ошибки дешифрования или соединения).

---

## 7. Диагностика и обслуживание службы

### Основные команды CLI mita на сервере
```bash
# Статус прокси-сервиса (RUNNING / IDLE)
mita status

# Активная конфигурация
mita describe config

# Статистика трафика и пользователей
mita get metrics
mita get users

# Журнал службы systemd
journalctl -u mita -n 50 --no-pager
```

### Управление через systemd
```bash
# Перезапуск службы
systemctl restart mita

# Остановка службы
systemctl stop mita

# Запуск службы
systemctl start mita
```

---

## 8. Откат и удаление (Decommissioning)

При завершении работы или необходимости полного удаления сервиса с VPS:

```bash
# 1. Остановка и отключение службы
mita stop
systemctl stop mita
systemctl disable mita

# 2. Удаление deb-пакета
dpkg -P mita

# 3. Удаление рабочих директорий и временных файлов
rm -rf /etc/mita /var/lib/mita /var/run/mita /tmp/mita-install

# 4. Удаление пользователя и группы (при необходимости)
userdel mita || true
groupdel mita || true
```

На локальном Mac при необходимости очистки тестового профиля:
```bash
rm -f ~/Library/Application\ Support/Mieru/vps-test-profile.json
```

---

## 9. Ограничения временного VPS

1. **Время жизни**: сервер предоставлен на ограниченное количество часов; после его выключения IP `31.58.213.192` станет недоступен.
2. **Отсутствие статического доменного имени**: подключение осуществляется напрямую по IPv4-адресу сервера.
3. **Минимальный профиль**: настроен транспорт `TCP` на одном непривилегированном порту (`28443`). Порт-диапазоны и UDP-over-underlay не активировались в рамках базового профиля.

## 10. Проверка через macOS-приложение

2026-11-11 подключение из debug Mieru.app с этим сервером прошло успешно: root TUN utun8, HTTPS без явного прокси вернул 31.58.213.192, загрузка example.com — HTTP200. После отключения root процессы завершились, utun8 исчез, AWG пользователя через utun7 продолжил работать. Подробности, необходимые клиентские исправления и границы проверки: [MACOS_SMOKE.md](MACOS_SMOKE.md). Сервер оставлен работающим; очистка из раздела 8 не выполнялась.
