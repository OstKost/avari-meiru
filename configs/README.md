# Общий профиль подключения

`profile.schema.json` описывает короткий JSON-профиль, общий для Desktop и Android. Примеры находятся в `examples/`; адрес `203.0.113.10` зарезервирован для документации, credentials фиктивны.

Обязательны `server` (IP или hostname), `server_port` (целое 1–65535), `transport` (`TCP` или `UDP`), непустые `username` и `password`. `traffic_pattern` необязателен и задаётся строкой стандартного base64 protobuf `TrafficPattern`. Для UDP клиент перед сборкой конфигурации должен разрешить hostname в IP вне туннеля: upstream mbox не разрешает доменное имя для UDP outbound.

Приложения преобразуют эти поля в outbound `mieru` ядра: `server`, `server_port`, `transport`, `username`, `password` и, если задано, `traffic_pattern`. Пользовательский профиль не является конфигурацией sing-box; прочие поля ядра задаются приложением по контракту `docs/core-contract.md`.

Ссылка upstream `mierus://user:pass@host?profile=...&port=...&protocol=TCP&traffic-pattern=...` представляет одну цель и может служить источником соответствующих полей после разбора. `protocol` — `TCP` или `UDP`; `traffic-pattern` — base64 protobuf. Формат `mieru://` кодирует полный protobuf `ClientConfig` и не импортируется в этот профиль. Импорт ссылок не входит в MVP. Не считайте `mieru://` форматом профиля приложения.
