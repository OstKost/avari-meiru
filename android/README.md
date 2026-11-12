# Mieru Android Client

Android 8.0+ (API 26+) VPN-клиент для протокола Mieru на Kotlin и Jetpack Compose, использующий официальное ядро `enfein/mbox` (rev `1c5b39718ebcd05e6adb31f95246fbf056be9bd7`) через AAR-библиотеку `io.nekohasekai.libbox`.

## Архитектура

- **UI / Презентация**: Jetpack Compose (Material 3), `MainScreen`, `MainViewModel`.
- **Профиль и безопасность**:
  - Единый сохраняемый профиль (`Profile`) с валидацией по `configs/profile.schema.json`.
  - Безопасное хранилище `EncryptedProfileStorage` на базе Android Keystore (`EncryptedSharedPreferences`, AES256-GCM).
  - Пароли и секреты исключены из резервного копирования (`allowBackup="false"`, `backup_rules.xml`, `data_extraction_rules.xml`).
  - Пароли никогда не выводятся в логи, отчёты и интерфейс в открытом виде (`SanitizedLogger`).
  - Поддержка импорта и экспорта JSON-профиля (экспорт скрывает пароль по умолчанию).
- **Служба VPN**:
  - `MieruVpnService` на базе `android.net.VpnService`.
  - Foreground Service с уведомлением и действием «Disconnect» (`PendingIntent`).
  - Разрешения Android: `BIND_VPN_SERVICE`, `INTERNET`, `ACCESS_NETWORK_STATE`, `CHANGE_NETWORK_STATE`, `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_SPECIAL_USE`, `POST_NOTIFICATIONS`.
- **Интеграция с ядром (`io.nekohasekai.libbox`)**:
  - `LibboxBridge` реализует реальные интерфейсы `PlatformInterface` и `CommandServerHandler`.
  - `Setup`: конфигурация рабочих и временных путей, лимитов памяти, версии.
  - `OpenTun`: создание туннеля через `VpnService.Builder`, установка MTU, адресов (`172.19.0.1/30` и `fdfe:dcba:9876::1/126`), DNS (`172.19.0.2` и `fdfe:dcba:9876::2` для DNS hijack ядра), полного диапазона маршрутов IPv4 `0.0.0.0/0` и IPv6 `::/0` (исключая обход).
  - **Владение файловыми дескрипторами**: ядро вызывает `dup(tunFd)` при открытии интерфейса; оригинальный `ParcelFileDescriptor` аккуратно закрывается службой после передачи.
  - **Защита сокетов**: `autoDetectInterfaceControl(fd)` вызывает `VpnService.protect(fd)` для исключения исходящего трафика ядра из туннеля.
  - **DNS и UDP**: запросы перехватываются правилом ядра `port: 53 -> hijack-dns` и направляются через upstream DNS `1.1.1.1:53` с detour `mieru-out`. Резолв доменного имени сервера перед подключением выполняется системным DNS вне туннеля.
  - **Сетевой мониторинг**: `ConnectivityManager.NetworkCallback` отслеживает смену сетей, назначает `setUnderlyingNetworks` по физическим адаптерам и вызывает `commandServer.resetNetwork()`.
  - **Сетевая проверка**: переход в статус `CONNECTED` происходит только после старта ядра и успешного ограниченного сетевого зондирования через туннель (таймаут 5 секунд). Неудача проверки переводит службу в `ERROR` с диагностическим сообщением.
  - Закрытие UI Activity не прерывает соединение; закрытие через Disconnect, `onRevoke` или `onDestroy` полностью останавливает ядро и освобождает ресурсы.

## 5 состояний интерфейса

1. `DISCONNECTED`: VPN отключён, ядро не запущено.
2. `CONNECTING`: подготовка VpnService, резолв DNS, старт ядра и проверка сетевого туннеля.
3. `CONNECTED`: ядро запущено, сетевой зонд подтвердил прохождение трафика.
4. `DISCONNECTING`: корректный останов ядра и закрытие дескрипторов.
5. `ERROR`: сбой валидации, ошибка запуска ядра или таймаут проверки туннеля.

## Сборка

Требования:
- Android SDK (API 35/36, Build-Tools 36.0.0): `/Volumes/KingstonM2/Android/sdk`
- JDK: Gradle требует JDK 17 или 21 (core builder использует Temurin 17 из `core/.cache/jdk/jdk-17.0.21+9/Contents/Home`)
- Gradle 9.3.1 (включён wrapper `gradlew`)
- AAR-библиотека ядра: `app/libs/libbox.aar` (архитектуры `arm64-v8a` и `x86_64`)

Подготовка AAR и сборка:

```bash
# 1. Скопировать готовый AAR из артефактов сборки ядра
cp ../core/dist/libbox.aar app/libs/libbox.aar

# 2. Настроить JAVA_HOME для Gradle (JDK 17+)
export JAVA_HOME="${JAVA_HOME:-$(/usr/libexec/java_home -v 17 2>/dev/null || /usr/libexec/java_home -v 21)}"
export PATH="$JAVA_HOME/bin:$PATH"

# 3. Собрать debug APK
./gradlew assembleDebug
```

## Расположение артефакта

- Debug APK: `android/app/build/outputs/apk/debug/app-debug.apk`
- Поддерживаемые ABI: `arm64-v8a`, `x86_64`
- Размер: ~127 МБ (включает native `libbox.so` для обеих архитектур)

## Установка на эмулятор или устройство

```bash
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n org.enfein.mieru/.ui.MainActivity
```

Подробные подтверждённые сценарии живой приёмки, команды и вывод runtime см. в [android/SMOKE.md](SMOKE.md).

