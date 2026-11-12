# План Mieru Admin: API и серверная TUI

Согласовано: Ubuntu 24.04 и Debian 13 x86_64/systemd, мастер установки и последующее управление. Пользователь уточнил: дальнейший desktop должен работать через API. Реализация разрешена новыми Antigravity workers после составления плана.

## Архитектура и результат

Новый независимый Go-модуль server/, бинарник mieru-admin. Слои: domain/service → mita/systemd/package adapters → HTTP API → TUI client. TUI не запускает mita/systemctl напрямую и не содержит отдельную реализацию правил конфигурации.

Использовать Go 1.25.5+ с фиксированным build toolchain в manifest, mita v3.38.0 и upstream API того же модуля. TUI — Bubble Tea v1.3.10; дополнительные компоненты только с pinned go.mod/go.sum. Язык интерфейса — русский, команды и API — английские. Поддержать клавиатуру, SSH PTY 80×24+, resize, Enter/Tab/Esc/Ctrl+C, маскированный пароль и восстановление terminal state.

## API и запуск

- Один бинарник: setup (установка локальной management-службы и вход в TUI), serve, tui, version/help. setup требует root, устанавливает собственный бинарник по фиксированному пути /usr/local/bin/mieru-admin и собственную systemd unit. Повторный setup не перезаписывает неожиданную чужую unit. TUI по умолчанию подключается к уже установленному API.
- API v1 слушает только 127.0.0.1:9081; публичный bind в v1 не поддерживается. Токен генерируется криптографически и хранится /etc/mieru-admin/token, root:0600. Передача — Authorization: Bearer, не argv/query/logs. TUI работает через HTTP, читая token-file под sudo/root. Будущий desktop использует SSH tunnel и тот же API; desktop сейчас не реализуется.
- Типизированный OpenAPI-контракт server/api/openapi.yaml: GET /v1/status, GET /v1/config (без паролей/хешей), POST /v1/installation, POST /v1/config/preview, PUT /v1/config, GET/POST /v1/users, PATCH/DELETE /v1/users/{name}, POST /v1/service (start/stop/restart), GET /v1/logs, POST /v1/profiles/export. Все запросы авторизуются. Ошибки имеют code/message без сырых stderr и секретов; start/stop/status различают systemd active и mita RUNNING.
- Ограничить request size и таймауты, журналировать только метод/путь/исход без request/response bodies, CORS не включать. Конфигурационные изменения сериализуются; preview возвращает revision, apply требует ту же revision и отвечает conflict при внешнем изменении. Отдельный HTTP API process остаётся активным после выхода TUI.

## Сценарии и работа с mita

- Первый мастер: preflight ОС/архитектура/systemd/root → проверить/установить pinned mita → внешний адрес → один TCP-порт (default 28443) → первый пользователь/пароль → preview → apply/start → readiness → сохранить client JSON. Не угадывать внешний адрес через сторонние сервисы: ввести явно или подтвердить предложенный адрес.
- Меню: состояние, порт, пользователи (добавить/сменить пароль/удалить), запуск/остановка/restart, последние очищенные логи, экспорт профиля. Удаление пользователя и применение изменений требуют конкретного подтверждения в TUI.
- Установка только при отсутствии mita: официальный mita_3.38.0_amd64.deb, встроенная проверенная SHA256 5170555aace38f76862541587b632d61848bba12f2e845b057977c9f9257e59a; apt устанавливает зависимости. Установленную неподдерживаемую версию не заменять автоматически. На чужом действующем сервере сначала read-only preflight, без изменений от самого открытия TUI.
- Использовать подтверждённые upstream-команды describe config, replace config <private-file>, start/stop/reload/status. Кандидат проверяется ValidateFullServerConfig pinned upstream; пароли хеширует штатный mita. Не писать integration по вымышленному mita check. Пакет имеет собственную systemd-службу: не создавать конкурирующий daemon mita.
- Изменения сохраняют остальных пользователей и неуправляемые поля существующей конфигурации. Для непрочитанного/неподдержанного формата отказаться от записи. Перед apply сохранять закрытый native backup с owner/mode, затем применить полный проверенный кандидат и reload/start по исходному состоянию. При ошибке вернуть конфигурацию и прежнее рабочее/остановленное состояние. Первичная установка пакета — отдельный шаг; не обещать автоматическое удаление пакета при ошибке конфигурации.
- Пароли только в памяти и закрытом временном файле, не в argv/logs. Отдельного plaintext vault нет. Для нового пользователя экспорт доступен сразу; позже пароль вводится повторно и сверяется штатным алгоритмом с серверным хешем. Export возвращает короткий профиль configs/profile.schema.json; TUI сохраняет его только по явному запросу в новый файл 0600, без вывода пароля на общий экран. traffic_pattern переносится только при подтверждённой upstream-поддержке.
- Firewall и SSH не менять: показать занятость порта/диагностику и нужные команды пользователю. Upgrades/uninstall, несколько новых портов, UDP-мастер, QR, fleet management, публичный API и desktop UI вне v1. Существующие дополнительные portBindings не удалять.

## Приёмка и поставка

Собрать один Linux amd64 бинарник с CGO_ENABLED=0, tar.gz с unit/инструкцией и SHA256SUMS; fixed dependencies и build manifest. Не требовать Go на VPS.

Тесты: HTTP auth/валидация/conflict, сохранение чужих полей/пользователей, rollback при apply/reload failure, маскирование ошибок; PTY мастер/пароль/Esc/Ctrl+C/resize. В одноразовых Ubuntu/Debian проверить actual installation, systemd/mita status, create/reset/delete user и client JSON; через pinned mbox с фиктивным профилем подтвердить реальный HTTPS и отказ неверному паролю. Ошибки/отмена до apply не меняют сервер; разрыв SSH/TUI не оставляет частично применённую конфигурацию.

Живой рабочий VPS не перезаписывать для тестов. На нём допустим read-only preflight; destructive сценарии и первичная установка — только disposable VM/container. Интеграция настоящего systemd проверяется в VM, container results так и обозначаются. Нет среды/сервера — отметить конкретный незакрытый сценарий. Итог: API contract, бинарник/TUI, инструкции setup/SSH tunnel, фактический smoke report.
