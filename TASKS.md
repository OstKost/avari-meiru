# Mieru MVP — очередь задач

## Новый этап: финальный Linux-выпуск и Mieru Admin API/TUI — ЗАВЕРШЁН

**Итог 12 ноября 2026: Все этапы нативной приёмки Linux Desktop и Server Admin API/TUI успешно выполнены.**

1. **Linux Desktop Native Acceptance (выполнено, exit code 0):**
   - Настоящая графическая сессия X11/Xfce + logind + `polkit-gnome` в Ubuntu 24.04 amd64 VM.
   - Все 7 тестов пройдены успешно:
     - Отмена диалога polkit (код 126 / dismissed, tun0 не создан, состояние disconnected).
     - Авторизация через polkit-gnome (рутовый helper поднят, интерфейс `tun0` UP `172.19.0.1/30`, таблица `2022` заполнена `via 172.19.0.2 dev tun0`).
     - Сетевой трафик через VPS `31.58.213.192:28443`: `https://example.com` HTTP/2 200 OK, внешний IP через `https://icanhazip.com` строго равен `31.58.213.192`, DNS hijack через sing-box core.
     - Переключение видимости окна через Ayatana D-Bus menu (скрытие/показ).
     - Отключение через трей (удаление `tun0`, полная очистка таблицы 2022, восстановление DNS, чистая остановка root helper).
     - Повторное подключение (reconnect) через трей с подтверждением аутентификации и трафика.
     - Аварийное завершение процесса GUI (`SIGTERM`): автоматический демонтаж `tun0`, сброс маршрутов таблицы 2022, восстановление DNS, отсутствие процессов-сирот.
   - Полный отчёт: [packaging/linux/ACCEPTANCE.md](packaging/linux/ACCEPTANCE.md).

2. **Server Admin API/TUI Native Acceptance под systemd (выполнено, exit code 0):**
   - Установка и автозапуск службы `mieru-admin.service` через `./mieru-admin setup --no-tui` в Ubuntu 24.04 VM под PID 1 systemd.
   - Управление `mita 3.38.0` (`/usr/lib/systemd/system/mita.service`).
   - Проверка fail-closed при повреждённой конфигурации (`LOAD_CONFIG_FAILED`).
   - Инициализация и создание пользователей (`POST /v1/users`), экспорт профилей (`POST /v1/profiles/export`), авторитетный Protobuf-файл `/etc/mita/server.conf.pb` (0600 root), автоматические снапшоты в `/etc/mita/backups/`.
   - Перезапуск службы mita через REST API (`POST /v1/service`), подтверждение прослушивания порта `*:28443`.
   - Полный отчёт: [server/SMOKE.md](server/SMOKE.md).

3. **Очистка и верификация окружения:**
   - QEMU VM корректно остановлена (`poweroff`), временные процессы завершены.
   - Тесты хоста macOS (`cargo test --workspace --offline` в `desktop/src-tauri` и `go test ./...` в `server/`) пройдены без ошибок (exit code 0).

Пользователь запросил два плана с Antigravity, выбрал локальные Linux-пакеты с настоящей desktop/pkexec приёмкой (без публикации/подписей), серверную TUI-мастер+управление для Ubuntu24.04/Debian13x86_64; затем явно разрешил выполнить оба плана НОВЫМИ Antigravity workers. Дополнение пользователя: TUI должна строиться для дальнейшего desktop управления через API. Исследователи завершены, их source не менялся.

Планы координатора: docs/LINUX_FINAL_PLAN.md и docs/SERVER_TUI_PLAN.md. Уточнения: Go service/domain API-first, localhost HTTPJSON v1 с токеном/SSH tunnel, TUI толькоAPIclient; никаких public API/firewall changes/plaintextvault/upgrades/uninstall вv1. Проверки настоящего systemd/logind нужны вVM; контейнер/Xvfb не выдавать за desktop. VPS вновь предоставлен на4часа, последний read-onlyTCP28443 probe успешен; перед network tests перепроверять, конфиг действующегоVPS длятестов не менять.

Run run_acafd03ce803:

- LF1 task_18c7a5756136 / ctx_2e6f58674517, НОВЫЙ Antigravity: packaging/linux/**, desktop/src-tauri/** и desktop/README.md; clean/fullbuild, pack-only/verify/manifests/checksums, standaloneArchrecipe, одна amd64UbuntuQEMUVM с systemd/logind/X11Xfce и настоящим pkexec, nativeGUI→VPS, cleanup. UI source/core/configs/общиедокументы/server не менять. Отчёт packaging/linux/ACCEPTANCE.md. QEMU отсутствовал — подготовка инструмента иVM входит вscope. Игнорируемые disk/key/guestcredentials; no hostnetwork/macOSVPNchanges.
- ST1 task_9ce11eaf370a / ctx_e6e9a77a5c28, НОВЫЙ Antigravity: запись только server/**, независимый Go module/APIv1/OpenAPI/TUI/setup/serve/systemd/tests/buildtar+checksums. РеальныеmitaAPI закрепленыv3.38.0; секреты неargv/logs; приватные временные candidate/backup+rollback, preserveexistingconfig, clientJSON schema. ДействующийVPS не менять. Disposabletestcontainers сначала; для настоящегоsystemd VM передаётся отLF1 ПОСЛЕ его desktopприёмки, не мутировать чужуюVM параллельно. Отчёт server/SMOKE.md.

ST1 сообщил OpenAPI/GoAPI/TUI/unit и Ubuntu/Debian container trafficHTTP200; native systemd запросил через msg_ef23359b511d. Координатор не принял контейнеры вместоVM, LF1 ещё работает, отправлен VMstatus/handoff запрос. После source review ST1 возвращены конкретные plan gaps: setup игнорирует systemctl/copy failures ипереписываетunit; --tokenargv; existingtokenpermissions; JSONmirror вместо authoritative native mita config/rollback иinactiveapply; forceinstallunsupportedversion; неверный journalctl -n string(rune); rawAPIerrors; отсутствиеgitignore/README; containerE2E долженfailneterrors ииспользоватьexportedprofile; realPTY вместотолькоmodelunit. Все fixes строго server/**, noVPSmutations. Доихзакрытия ST1неготов.
ST1 сообщил8fixes+relatedtests/container+livePTYpass иожидаетVM. Второйtargetedreviewобнаружилоставшиесяactual gaps: LoadConfigдаётJSONprecedence/корруптныйPBfallbackdefault, manualToProtoтеряетschemafields, SaveConfigигнорируетWriteNativePbошибку; переданы3 integrityfix иконкретныеregressioncases безрасширенногоaudit. LF1 QEMU11.1.3 установлен, pack-onlyожидаетArch; VMstatusзапрошенповторнопослеreadtranscript. FullUIbuildраннеепоказалsphere.tsargmismatch изизмененийдругогоконтекста — reuseстарогоdistнепринимать заcleanrelease; LF1 поручено сообщитьточныеunresolvedUIgaps, не менятьчужуюобласть. ПокаVMнепередана, ST1неиспользуетеё.
ST1 сообщилsourceintegrityfixes/regression4cases PASS, PTY/containerE2E/build/checksums обновлены, ждётVM намеренно. LF1 VMready: одноразоваяUbuntu24.04amd64 QEMUTCG сsystemd/logind/X11Xfce:0, polkit-gnomeagent обычногоtestguest. Координатор поручилсначала закончитьnativeclient/pkexec/VPS приёмку, затемпередатьendpoint+privatecredential paths ST1; VMпокаещёнесвободна, параллельныхзаписейнет. Userдоп.действийпокане требуется.

Координатор владеет корневыми документами, docs/** и последовательнойVMпередачей/интеграцией. Не более3 исполнителей; моделиAntigravity текущие, своихworkersне запускать. После source/runtimefix пересобирать только относящиеся артефакты, успех = фактически пройденные проверки. Прошлый этап и его ограничения ниже сохранены как история.

## Активный этап Android / Linux (уточнение пользователя)

**Итог 2026-11-12: этап реализации завершён.** Android debug APK arm64/x86_64 прошёл живую приёмку на Android16 arm64 AVD с VPS до23:25МСК. Linux GUI/helper прошли Xvfb/localmita connect/DNS/HTTPS200/disconnect/reconnect/tray show+quit/ownerdeath cleanup; финальный health watchdog ready/cleanup0. Ubuntu24.04/Debian13/Arch пакеты обновлены из принятой release сборки, все payload hashes совпали, policy/modes проверены; install/remove и ldd приняты. MacOS cargo check --workspace --offline0. Активных workers и контейнеров проекта после приёмки нет; Grok402 обработан coordinator takeover без повышения модели. Результаты: README.md, android/SMOKE.md, docs/LINUX_SMOKE.md, core/LINUX_SMOKE.md, packaging/linux/README.md. Ограничения: physicalAndroid/API26/x86_64 runtime и настоящий Linux desktop/logind auth dialog, IPv6/UDP/долгая стабильность отдельно не проверены. История ниже сохраняет предыдущие неудачные попытки и не является текущим статусом.

Пользователь разрешил реализацию Android и Linux до рабочего результата через Orca workers; macOS MVP служит рабочей базой и не должен регрессировать. Этот раздел имеет приоритет над историческими macOS-only ограничениями ниже. Windows и iOS отложены.

Цели: Android API 26+, arm64-v8a и x86_64, Kotlin/Compose, ручная установка debug APK; Linux x86_64 Ubuntu 24.04 LTS, Debian 13 и актуальный Arch, общий Tauri UI + системный VPN, .deb и PKGBUILD без публикации. Профиль configs/ общий, mbox commit сохраняется. Локальные Android emulator/Linux VM готовить по возможности; доступность временного VPS перепроверять. Непроведённые live проверки не заменять сборками.

Run: `run_6b8dccf09c97`.

Срочный приоритет пользователя: временный VPS доступен только до 2026-11-11 23:25 Europe/Moscow (20:25 UTC); следующий сервер будет через день. До дедлайна Android live VPN и доступные Linux core сетевые проверки приоритетнее документации/упаковки. Оба активных исполнителя уведомлены в 23:07 МСК. Проверку Linux sidecar/proxy не выдавать за приёмку ещё не реализованного Linux GUI.

- AL1 `task_5eeebfd602bf`, Grok, dispatch `ctx_114b48c530b3`: запущен. Запись core/**: Linux sidecar, Android AAR, подтверждённый core/ANDROID_CONTRACT.md. Единственный владелец установки общего Android SDK/NDK/JDK для сборок.
- AL2 `task_905395e70019`, Antigravity, dispatch `ctx_c906f094da44`: запущен. Запись android/**: проект/UI/VpnService по реальному API. Независимая реализация параллельна AL1; финальная интеграция и assembleDebug требуют настоящего AAR AL1. При ожидании — ask и завершить ход, без polling.
- AL3 `task_2b8e3de4c07b`, Grok: зарегистрирован, зависит от AL1 и свободного слота. Запись desktop/src-tauri/** и desktop/README.md: Linux helper/pkexec/IPC и lifecycle без регрессии macOS. GUI non-root; только фиксированное установленное ядро, проверенный профиль, ограниченный IPC. Linux process identity и cleanup вместо macOS API.
- AL4: после AL3 отдельная небольшая задача упаковки Linux (Codex gpt-6-luna low после проверки доступности): .deb для Ubuntu/Debian, PKGBUILD для Arch, fixed root-owned helper/core paths, policy, desktop entry, лицензии. Область упаковки согласовать после AL3; backend параллельно не менять.
- AL5: координатор — интеграция и фактическая приёмка Android/Linux: разрешения, HTTPS/внешний IP, DNS, IPv6 без обхода VPN, повторное подключение, отключение/выход/аварийная очистка и восстановление сети. connected требует готовности ядра и сетевой проверки. Зафиксировать артефакты и проверенное/непроверенное; ошибки обязательной сборки или cleanup означают неготовность.

Android build milestone: AL2 сообщил assembleDebug exit 0 и настоящий APK с arm64-v8a/x86_64. Сетевой результат пока не принят. После просмотра кода координатор вернул Antigravity follow-up через ctx_c906f094da44: гарантированный onDestroy cleanup, независимые closeService/close, fail-closed protect, реальные interface callbacks, DNS/IPv6 и runtime-приёмка на существующем AVD Avari_API_36. Локальный AAR присутствует в core/dist и android/app/libs, но AL1 ещё выполняет проверку происхождения и документирует контракт. Внешний UI worker больше не активен; Linux backend ожидает AL1.
Android runtime follow-up зарегистрирован Orca отдельно: task_65fd9967fb85, активный dispatch ctx_b563b4f7540c (тот же Antigravity workspace/terminal).

AL1 завершён: build-linux.sh exit0, Linux x86_64 ELF и check в Docker Ubuntu24.04 успешны; build-android.sh итоговый exit0, AAR arm64-v8a/x86_64, реальные API через javap, core/ANDROID_CONTRACT.md готов. Живой трафик пока не проверен. Grok получил срочный follow-up Linux sidecar proxy/TUN live до отключения VPS, запись только core/**, ожидается core/LINUX_SMOKE.md. AL3 запущен: Grok dispatch ctx_b3fd6e228231, scope desktop/src-tauri/** и desktop/README.md. Пути установки согласованы: /usr/lib/mieru/mieru-helper, /usr/lib/mieru/sing-box, /usr/share/polkit-1/actions/app.mieru.policy. Android worker получил готовый контракт и путь JDK17.
Linux live follow-up task_1ecb3607716a / ctx_ca4919bd9f2d завершён к23:16 МСК: в isolated Docker Ubuntu24.04 amd64 proxy и настоящий TUN дали HTTPS200, IP31.58.213.192, example.com200. После SIGTERM tun0 исчез, ip rule и ip route show table all совпали baseline; контейнеры удалены. Доказательства core/LINUX_SMOKE.md, переданы AL3. Это подтверждает Linux core/TUN, но не ещё разрабатываемый GUI/helper, UDP и IPv6. Android live остаётся срочным до23:25.
DNS gap закрыт к23:22 МСК: task_8686c8dc40a8 / ctx_6c5f1d8f8910, Docker TUN с dns_mode hijack и DNS172.19.0.2 только внутри контейнера. nslookup подтвердил DNS, журнал — outbound UoT к1.1.1.1:53 через mieru; HTTPS по hostname без --resolve дал200/IPVPS. Отчёт core/LINUX_SMOKE.md дополнен и передан AL3. IPv6 и GUI остаются непроверенными.
Android runtime принят по android/SMOKE.md: assembleDebug exit0, APK arm64-v8a/x86_64; на AVD Avari_API_36 подтверждены baseline109.107.176.224 → VPS31.58.213.192 по HTTP/HTTPS, DNS, сохранение VPN после закрытия Activity, disconnect с удалением tun0 и возвратом baseline, повторный connect/stop. Исправлены перечисленные cleanup/protect/interface callbacks gaps. Физический Android, API26 и x86_64 runtime отдельно не проверены. Android готов как emulator-tested debug MVP; Linux GUI/helper/упаковка продолжаются AL3/AL4.
Android документация завершена отдельным worker_done task_1b08197d4cb6 / ctx_9694078f9662: Android16/API36 arm64, точная схема DNS hijack, воспроизводимый AAR copy/JAVA_HOME, live окно23:21–23:24МСК и финальный disconnected без tun0. AL4 зарегистрирован task_d7a5f4fc5a84, зависит от AL3 task_2b8e3de4c07b; запись только packaging/linux/**, планируемый недорогой Codex gpt-6-luna low. AL3 worker жив и работает; новые backend workers не запускать.

2026-11-12: AL3 завершён по build/test: macOS workspace tests0 (26+3+19), Docker amd64 shared/helper tests0 (20+3), release helper build0 без WebKit/GTK, Linux cargo check desktop0 на rustc1.98.1. Live helper/GUI не проверен, поэтому общий Linux результат ещё не принят.
AL4 запущен: ctx_8f73f6ade721, Codex gpt-6-luna low (модель подтверждена локальным cache и receipt), запись packaging/linux/**. Backend runtime follow-up task_58b8d148ddda / ctx_33edb0d25325 запущен Grok в прежней backend области: установленный helper/pkexec/IPC, cleanup/DNS restore, GUI smoke. Старый VPS не использовать; разрешён отдельный локальный mita server с фиктивными credentials в isolated Docker для дальнейшего E2E. Не переносить simulated/Xvfb smoke на полноценный Linux desktop/auth dialog. Пакеты финализировать после относящихся runtime fixes, не дублировать cargo volumes и не править чужие файлы.
AL4 преждевременно остановился без worker_done, создав только .gitignore и сославшись на macOS host, хотя собственный docker info успешен. Координатор прочитал transcript и отправил продолжение тому же Task/Dispatch и той же gpt-6-luna low: выполнять dpkg-deb/makepkg и Rust GUI build внутри isolated amd64 Docker, сначала создать реальные рецепты, сообщать только конкретные ошибки. Модель не повышалась, задачу выполненной не считать.
AL4 попытка ctx_8f73f6ade721 закончилась failed: recipes созданы, pnpm build0, но multi-GB Docker context не дошёл до release build; пакетов нет. Retry той же task_d7a5f4fc5a84 запущен ctx_34c4f5421ef0, та же gpt-6-luna low. Переданы конкретные исправления: allowlist staging без target/cache вместо ROOT context, host-built UI dist (в Dockerfile не установлен pnpm), корректные mounts scripts, debian:13 вместо debian:debian, отдельный PKGBUILD и реальные install/remove/ldd проверки. Финальные пакеты ещё не приняты.
AL3 runtime follow-up task_58b8d148ddda завершён: установленный helper root-owned, обычный IPC uid, локальный mita3.38.0 с фиктивными credentials, connected/TUN route, HTTPS200, disconnect/IPC loss/owner death очистили TUN и восстановили DNS (file и symlink). Проверены pkexec no-agent/Result.NO, но не happy-path dialog. Policy allow_gui удалена, rollback частичного resolvectl добавлен (runtime эта ветка не исполнялась), qemu/Rosetta child identity исправлена. Подробно desktop/README.md. Linux GUI пока не запускался. Grok получил следующую задачу Xvfb/DBus GUI smoke с reuse ELF упаковщика, без дублирования долгого cargo build; Codex уведомлён о финальном backend snapshot и запросе GUI artifact path.
AL4 retry ctx_34c4f5421ef0 добился release cargo build0 и staging43MiB. GUI находится packaging/linux/dist/build/out/usr/bin/mieru (путь без out в worker final был неверным). Ubuntu/Debian deb созданы, Ubuntu install/remove прошёл; Debian repeat после dependency fix и Arch install/remove не завершены. Worker снова остановился без worker_done; отправлено продолжение без rebuild GUI с точными fixes: pacman download sandbox только disposable container, правильный PKGBUILD srcdir, UI build до staging, finish Debian/Arch checks. Готовый ELF передан Grok GUI follow-up task_6b507130da68 / ctx_2e9ba1d15d2e. Linux пакеты пока не приняты окончательно.
AL4 дополнение: Debian13 install/remove и ldd GUI теперь0; Arch основной artifact mieru-0.1.0-1-x86_64.pkg.tar.zst собран после исправления произвольного выбора debug package. Arch install/Q пока непонятен по worker отчёту. Координатор запустил только недостающую integration проверку существующего Arch artifact (без rebuild) в container mieru-coordinator-arch-check, exec session76915; pacman cache volume mieru-arch-integration-cache. Codex подтвердил паузу команд и изменений до результата координатора, intentional idle escalation не требует повторного запуска.
Координаторская Arch integration завершилась exit0, ARCH_INTEGRATION_OK: pacman install0, Q mieru0 (0.1.0-1), ldd GUI без not found, root755 GUI/helper/core, policy на месте, remove0, Q после remove nonzero. Контейнер --rm, без host-network, pacman syscall sandbox workaround только внутри disposable container. Все3 package install/remove checks приняты. Codex получил только docs/debug artifact cleanup и обязательный worker_done без повторных тестов. Остаётся GUI Xvfb runtime, настоящий desktop/auth dialog отдельно не подтверждён.
AL4 worker_done succeeded получен, README обновлён/debug artifact удалён. Однако GUI follow-up task_6b507130da68 завершился failed: Xvfb окно Avari Mieru420x720 и profile0600 подтверждены, no-agent ошибка корректна, но под test-only polkit YES GUI helper_identity отклоняет exe Rosetta до Connect; упакованный helper также старее runtime fix. Это не успешный GUI VPN. Grok получил продолжение: строгий interpreter-aware helper identity с nonce/owner/PID/UID/starttime, проверки и немедленный source-ready status; затем координатор закажет единую пересборку GUI/helper и всех3 пакетов, после чего повторится GUI network/cleanup smoke. Старые пакеты содержат проверенные по установке, но ещё не финальные binaries; не выдавать за завершённый Linux MVP.
Grok source-ready task_8a3609253ff3 / ctx_e5c8ed258a0c: native exe строго HELPER_PATH; interpreter fallback только root uid/PID/starttime + exact argv HELPER_PATH/nonce/GUI pid; shared lib21tests0, Linux sidecar identity test0. Упаковщику отправлен follow-up cached rebuild GUI/helper+3 пакетов с source manifest и hashes, немедленным status после нового out. На вопрос msg_e7c0bec11268 ответили: ждать новый ELF, не дублировать release build и не использовать старый snapshot. GUI runtime/новые пакеты ещё ожидаются.
Refreshed build task_1ecdacdcf008 / ctx_f0d2c3b90d48: release build0 на текущем backend, UI reused. GUI dist/build/out/usr/bin/mieru sha256960179b8162d49c9d67e14dd3a65f6945cbd15e70f717368345daf05a5da3db0; helper dist/build/out/usr/lib/mieru/mieru-helper sha256f29ac063e8fd91a5de5f02f4ab092aa29980a5adccb683d3035f85f297ad44ff. Полный prefix packaging/linux/. Пути и hashes переданы Grok для продолжения GUI smoke; Debian/Arch packages обновляются отдельно. Intentional idle Grok завершён отправкой нового ELF, новых GUI build не запускать.
Refresh follow-up reported failed: Ubuntu/Debian обновлены, Debian payload hashes совпали; Arch hashes отличались, а expected final pacman-Q absence интерпретирован как ошибка. Координатор прочитал PKGBUILD: !debug есть, !strip нет (makepkg меняет bytes). Упаковщику отправлено только Arch repack с !strip, явные installed/removed markers и байтовые hashes. Rust/дебы/UI повторно не собирать. Это не доказательство устаревшего кода в Arch до разбора strip; refreshed Arch пока не принят.
Arch repack task_6aebbf65e495 / ctx_de4260dbc9a3 succeeded: !debug !strip, GUI/helper payload hashes совпали с release, INSTALLED/REMOVED markers прошли. Однако GUI next smoke нашёл /proc/root-helper/exe EACCES обычному uid; новый source fallback пока ждал rebuild. Координатор до rebuild выявил ещё native-EACCES rejection и отсутствие kernel SO_PEERCRED проверки server в GUI. Grok получил ответ на msg_c4d172b2f590: добавить strict native/interpreter argv fallback и uid0/PID peer credentials + минимальные tests, затем source-ready. Меняем порядок: build-only GUI/helper → успешный GUI smoke → финальная упаковка3 пакетов. Пакеты старого snapshot пока не финальный Linux результат.
Source-ready msg_f43a6fb9a140: strict argv0 native/helper или translator+helper; adjacent --session nonce и --owner-pid GUI pid, root uid/PID/starttime/root-owned install сохранены. GUI проверяет SO_PEERCRED uid0 и pid==Hello.pid. shared22tests0; vpn_linux.rs host macOS не компилировал. Codex получил новую build-only задачу через ctx_de4260dbc9a3: refreshed staging/UI reuse, Linux release GUI/helper, hashes+manifest, никаких пакетов до успешного GUI smoke. Grok intentional idle уведомлён ждать новый ELF.
Build-only task_497f6a75a83f / ctx_eb6d9c5801f2 succeeded: current Linux cargo release0, GUI b824cac0a7f7c70671ae8d1996853672c55616e083f4a638235fede01ed2d1e4, helper61041715953adf43191d6c86ecfaeb43c163deaaa59c5b19d1363856471b469d. Пути packaging/linux/dist/build/out/usr/bin/mieru и out/usr/lib/mieru/mieru-helper; SOURCE-MANIFEST.txt/ELF-SHA256SUMS.txt в dist/build. UI reused, packages intentionally old. Grok получил hashes и продолжает финальный GUI smoke, release/packages пока не дублировать.
На b824/6104 GUI smoke подключился через pkexec testYES/localmita: UI connected, DNS172.19.0.2, HTTPS example.com200, secret leak check0. Disconnect фактически снял TUN/table2022/helper и восстановил DNS, но GUI считал zombie pid alive20s и сохранил stale session, reconnect blocked. Grok исправил proc stat Z/X dead + owned child reap; shared23tests0, Linux vpn_linux.rs пока не компилировался. В ответ msg_2f5d89c3c348 заказана следующая build-only через упаковщика, без UI/core/packages. Требуется повторить GUI disconnect/reconnect/hide-show/owner-death на новом snapshot, packets прежнего поколения не финальные.
Grok получил402 Payment Required: usage balance exhausted. Модель не повышалась, дорогое fallback не запускалось. Backend source fix готов, Codex build-only task_b694111851bd / ctx_8f2ba9d5afd9 продолжает работу. Координатор берёт оставшуюся GUI integration на себя (gap/reported failure), используя /tmp/mieru-gui-smoke.sh, /tmp/mieru-gui-cdp.py, image mieru-gui-xvfb и local mita container mieru-gui-mita/network mieru-al3-net. Скрипт старого smoke требует новых hashes; для hide теста подготовлен WM_DELETE sender в /tmp/mieru-coordinator-wm-close.py вместо уничтожения X window. Исходники backend не меняются координатором до новой runtime evidence.
Build-only zombie snapshot succeeded: GUI5a34bbb7f1e6481d9bf446e9975700124af6741b919d72426c10bf5ee302fd39/helperc6df8524943c19be5bb5fec678014b0583d0137d829ee6936cf921a40a23057d. Coordinator smoke session19547: connect+DNS+HTTPS200, disconnect CLEAN, reconnect, WM_DELETE hide, ownerdeath CLEAN/DNS restore иLEAK_OK прошли. Wrapper1 только из-за show via raw XMapWindow (не команда приложения) — этот пункт не принят, требуется tray DBus action. UI health ошибочно пишет Ядро не найдено: shared sidecar_path не ищет установленный fixedLinuxPath. Codex получил маленький health-path source fix (scope только shared/core_config.rs + packaging README) и build-only, никаких пакетов до итогового smoke. Grok blocked by402, дорогого fallback нет.
Installed-path fix source23tests0/build-only0: GUI2c6c56ca36ffe2bb11d27bb4c2548c7b730c667030e03615719861b317fd1c97/helper15226e772787850e2bc52ab7467c1583c85db6cbb76707491b7789e212bd8f47. Координаторский final GUI smoke session98594 WRAPPER0: HTTPS200, disconnect/reconnect CLEAN, WM_DELETE hide, native DBusMenu Show → IsViewable, ownerdeath CLEAN/DNS restore, relaunch+connect+native DBusMenu Quit CLEAN, LEAK_OK. Xvfb использовал test-only watcher/YES rule, не настоящий desktop/admin dialog. Отдельно health оставался ошибочным (Core stopped). Прямой shell эксперимент подтвердил: Ubuntu dash cat<&0 background сразу EOF, exec3<&0 + cat<&3 сохраняет pipe; Bash оба ALIVE. Codex получил targeted health.rs watchdog FD-preservation fix и build-only; только affected health runtime остаётся перед финальной упаковкой.
Финальный health snapshot GUI1f425262582d3eba9f7aa91b38f8f52276118e68d8de5f4e2eb273840159cef7/helper15226e...: focused watchdog/EOF tests0, Linux build-only0. Координатор session82467 HEALTH_GUI_WRAPPER0: healthready latency156ms, UI Готов к подключению, noTUN/noDNSchange, после GUIkill HEALTH_CORE_ABSENT/LEAK_OK. Полный GUI сценарий не повторялся после isolated health fix. Host cargo check --workspace --offline0 (9.81s). Итоговый отчёт docs/LINUX_SMOKE.md. Grok task_8a3609253ff3 завершён координатором по собственной runtime evidence (история402 сохранена), без дорогого fallback. Local mita container остановлен/удалён0, временный fake пароль удалён; macOS сеть не менялась. Финальная repack3 задача task_6010e0d9e68b / ctx_cb98a75057d5 запущена Codex: только упаковка из проверенного out, без compile/UI/core, payload hash checks3 и policy/modes. До её результата пакеты прежнего поколения.
Orca сначала отказал task-update completed, пока Grok dispatch active. Координатор подтвердил exact owned idle worker, guarded worker-stop ctx_e5c8ed258a0c вернул stopped/closed_agent_terminal0, после чего task-update завершён по собственной интеграционной evidence. Повторные402 не ретраятся. Финальные Ubuntu/Debian payloads уже подтверждены DEB_PAYLOADS_OK: GUI/helper hashes финального snapshot, policy noallow_gui, root755; Arch repack ещё выполняется.

Внешняя UI задача `task_94ba498f7c71` / `ctx_756c04efe6cc` активна в другом Run, учитывается в лимите трёх исполнителей. Исполнитель подтвердил область только desktop/ui/src/main.ts, desktop/ui/src/style.css, desktop/ui/README.md; core/android/backend/общие документы не меняет. Не дублировать её проверки/реализацию.

Все workers используют фактический TUN mbox, не пишут собственный сетевой стек, не печатают секреты и не трогают существующий пользовательский VPN macOS. QR, stores, AUR-публикация, always-on, split tunneling и сложный reconnect вне этапа.

## Текущий этап

Пользователь разрешил реализацию первого MVP для macOS. /init завершён. Активный путь: T0 → T1/T2 → T4 → T5 → интеграция macOS. Android (T3) и Windows отложены. VPS будет предоставлен позже; отсутствие сервера не блокирует сборку, UI и локальные проверки. Проверку реального VPN-трафика отмечать как ожидающую сервер.

Это уточнение имеет приоритет над исходными формулировками задач, в том числе сохранёнными в Orca: T0 проверяет macOS TUN, привилегии, sidecar и контракты; Android API/AAR и Windows пока не исследовать. T1 собирает macOS sidecar, без AAR и Windows. T6 для macOS не ожидает отложенный T3; координатор зарегистрирует отдельную задачу интеграции macOS после T5.

`PLAN.md` сохраняется как исходный документ. Уточнения: Desktop TUN принадлежит mbox; Rust управляет ядром. Для первого результата достаточно debug APK и локального Desktop-приложения. Windows остаётся целевой платформой; отсутствие Windows-хоста явно фиксируется как ограничение проверки.

## Правила запуска

- Прочитать `AGENTS.md` и соответствующий раздел ниже. Координатор запускает только задачи с завершёнными зависимостями; регистрация задачи не означает её запуск.
- Исполнители: Grok — ядро/backend; Antigravity — Android; Codex `gpt-6-luna --effort low` — небольшие задачи; T6 выполняет координатор.
- У Grok и Antigravity сохраняются текущие настройки модели. При недоступности агента/модели задача блокируется, без автоматического дорогого fallback.
- Максимум три исполнителя. Общие документы меняет координатор; внешние зависимости фиксируются по ревизии. Рабочий каталог текущий, пока области записи не пересекаются.
- В каждом отчёте: результат, файлы, выполненные команды с исходом и ограничения. Non-zero обязательной проверки означает неготовность задачи; отсутствие среды помечается как непроверенное.
- T0 → T1 и T2 параллельно → T3 и T4 параллельно → T5 после T4 → T6 после T3 и T5.

## T0 — подтвердить ядро и контракты

Исполнитель: Grok. Зависимости: нет.

Цель: проверить пригодность `https://github.com/enfein/mbox` как общей основы Desktop и Android до реализации приложений.

Разрешённая запись: только `docs/core-contract.md` и временные/игнорируемые артефакты исследования. Общие файлы и код приложений не менять.

Работа:
- Выбрать конкретную upstream-ревизию с Mieru, зафиксировать URL, commit, требования toolchain, build tags и фактические команды сборки Desktop/libbox.
- Проверить по исходникам и минимальной сборке реальные API Android AAR: создание службы, передача TUN fd, platform callbacks и socket protect. Не полагаться на `libbox.BoxService` из исходного PLAN.md.
- Подтвердить путь TUN для macOS/Windows, права запуска и исключение серверного соединения из туннеля. Зафиксировать механизм освобождения ресурсов при остановке.
- Описать общий профиль, конвертацию в конфигурацию ядра, минимальные команды Tauri управления профилем/подключением и событие `vpn-status`.
- Контракт состояния: disconnected, connecting, connected, disconnecting, error; определить подтверждение готовности ядра и источник ошибки. Не считать сам запуск процесса подключением.
- Проверить upstream-формат ссылок, ограничения TCP/UDP и требования распространения выбранного ядра. Не обещать неподтверждённые возможности.

Результат: документ с закреплённой ревизией, подтверждёнными API, командами и выводами о пригодности. Если критическая возможность не подтверждена, сообщить блокер координатору; зависимые задачи не запускать. Не заменять архитектуру молча.

Проверка: минимальная сборка/проверка конфигурации в доступной среде; проверки чужих платформ отмечать отдельно. Non-zero обязательной проверки означает незавершённую задачу, кроме явно документированного внешнего блокера.

## T1 — воспроизводимая сборка ядра

Исполнитель: Grok. Зависимости: T0.

Цель: предоставить артефакты ядра для обоих приложений по `docs/core-contract.md`.

Разрешённая запись: `core/**`. Корневые документы и приложения не менять.

Работа:
- Подготовить скрипты получения закреплённой ревизии и сборки sidecar macOS arm64/amd64, Windows amd64 и Android AAR для arm64-v8a, armeabi-v7a, x86_64.
- Описать toolchain, команды, имена и расположение артефактов, необходимые лицензии. Не коммитить большие бинарники и действующие конфиги; временные загрузки исключить локальными правилами внутри core.
- Предоставить минимальную конфигурацию для проверки запуска на loopback и понятные ошибки отсутствующих инструментов.

Результат: скрипты и `core/README.md`, локальный sidecar и доступная сборка AAR. Неподтверждённые целевые сборки обозначить, не выдавать за выполненные.

Проверка: сборка локального sidecar, принятие минимального конфига штатным валидатором ядра и сборка AAR при доступном SDK. Non-zero обязательной команды означает неготовность; внешние блокеры сообщить.

## T2 — общий профиль и конфигурации

Исполнитель: Codex, модель `gpt-6-luna`, effort `low`. Зависимости: T0.

Цель: дать приложениям общий формат профиля и проверяемые примеры на основании `docs/core-contract.md`.

Разрешённая запись: `configs/**`. Ядро и приложения не менять.

Работа:
- Описать JSON-профиль: server, server_port, transport, username, password, необязательный traffic_pattern; точное отображение в ядро согласовать с утверждённым контрактом.
- Добавить схему и примеры с фиктивными credentials для Desktop/Android. Не подменять пользовательский профиль полным произвольным конфигом ядра.
- Описать минимальную валидацию и подтверждённый upstream-формат ссылок. Не реализовывать неподтверждённый `mieru://`.

Результат: схема, примеры и инструкция импорта; никаких действующих секретов.

Проверка: JSON разбирается, примеры соответствуют схеме и закреплённому контракту. Не создавать расширенную тестовую инфраструктуру. Ошибка проверки означает неготовность.

## T3 — Android MVP

Исполнитель: Antigravity. Зависимости: T1, T2.

Цель: рабочий debug APK на Kotlin + Compose с общим ядром, по `docs/core-contract.md`, `core/README.md` и `configs/`.

Разрешённая запись: `android/**`. Другие области не менять; вопросы по контрактам отправлять координатору.

Работа:
- Создать Gradle-проект с wrapper, подключить собранный AAR без коммита бинарника.
- Реализовать экран профиля, ручной ввод/JSON, локальное сохранение и кнопку подключения со статусом/ошибкой; секреты не выводить в логи.
- Реализовать VpnService.prepare, foreground notification, запуск/остановку libbox, передачу fd и protect callbacks по реальному API. Исключить петлю маршрутизации.
- Освобождать fd и службу при отключении/ошибке. QR-сканирование отложить.

Результат: debug APK и `android/README.md` с командами и ограничениями.

Проверка: `./gradlew assembleDebug`; при наличии устройства — запуск и запрос разрешения. С рабочим сервером — подключение, загрузка страницы, внешний IP и отключение. Non-zero сборки означает неготовность; отсутствие устройства/сервера явно фиксировать.

## T4 — Desktop backend

Исполнитель: Grok. Зависимости: T1, T2.

Цель: Tauri v2/Rust backend для управления TUN ядра по `docs/core-contract.md`, `core/README.md` и `configs/`.

Разрешённая запись: `desktop/src-tauri/**` и `desktop/README.md`. UI и общие документы не менять; необходимый минимальный frontend создаётся отдельно в T5.

Работа:
- Создать Rust/Tauri проект, конфигурацию sidecar и необходимые capabilities с минимальными разрешениями.
- Реализовать хранение/валидацию профиля, команды управления и `vpn-status`; ошибки возвращать UI без секретов.
- Реализовать жизненный цикл sidecar, готовность, обработку неожиданного выхода, привилегии и очистку сетевых ресурсов по утверждённому механизму. Не разрабатывать отдельный сетевой стек Rust.
- Зафиксировать точные команды и payload для T5. Обеспечить единственный экземпляр управляемого ядра и исключение серверного соединения из туннеля.

Результат: backend и контракт интеграции UI в `desktop/README.md`.

Проверка: `cargo check` из `desktop/src-tauri`, запуск/остановка ядра на macOS доступным способом, восстановление сети. До появления UI допустима backend-проверка; запуск всего приложения завершает T5. Windows без соответствующего хоста остаётся непроверенным. Non-zero обязательной проверки означает неготовность.

## T5 — Desktop UI и трей

Исполнитель: Codex, модель `gpt-6-luna`, effort `low`. Зависимости: T4.

Цель: минимальный интерфейс рабочего Desktop VPN по `desktop/README.md` и утверждённому контракту.

Разрешённая запись: `desktop/ui/**`, `desktop/src-tauri/src/tray.rs` и необходимые точки подключения трея/настройки frontend в `desktop/src-tauri/**`. T4 должен быть завершён до этих изменений. Остальной backend не переписывать; несовместимость сообщить координатору.

Работа:
- Создать TypeScript/Vite/Tailwind frontend, pnpm scripts и lockfile; добавить профиль с ручным вводом/JSON, кнопку, статус и очищенные логи.
- Интегрировать команды/события Tauri; корректно показывать переходные состояния и ошибки.
- Добавить трей: подключить/отключить, показать окно, выход. Закрытие окна оставляет VPN активным, выход останавливает ядро и завершает приложение.
- Документировать запуск в `desktop/ui/README.md`. Не добавлять тяжёлую UI-инфраструктуру и расширенные функции.

Результат: запускаемое приложение с согласованным backend и треем.

Проверка: `pnpm build` из `desktop/ui`, `cargo check` и запуск Tauri на macOS; пройти профиль → подключение/ошибка → отключение → выход. Без действующего сервера соединение считать непроверенным. Ошибка обязательной команды означает неготовность.

## T6 — интеграция и передача

Исполнитель: координатор. Зависимости: T3, T5.

Цель: собрать результаты без расширения тестирования и точно описать готовность MVP.

Разрешённая запись: общие документы и минимальные интеграционные исправления после завершения исполнителей. Крупные исправления возвращать ответственному агенту.

Работа:
- Проверить совместимость артефактов ядра, профилей и приложений; устранить фактические интеграционные ошибки.
- Подготовить корневой README с командами, местами артефактов, требованиями прав и известными ограничениями.
- Суммировать доказательства исполнителей. Повторять проверки только при изменениях, пробеле или противоречии в отчёте.
- Проверить подключение к реальному серверу, страницу/внешний IP, отключение и восстановление сети; Desktop — также выход из трея и отсутствие оставшегося процесса.
- Windows проверять на Windows-хосте; без него не заявлять подтверждённую работоспособность. Аналогично фиксировать отсутствие Android-устройства или серверного профиля.

Результат: доступные debug-артефакты, инструкция запуска и честный перечень проверенного/непроверенного. Non-zero обязательной сборки или сломанное восстановление сети означает неготовность соответствующего результата.

Вне текущего этапа: iOS, QR, бенчмарки, полный аудит DNS/IPv6-утечек, матрицы совместимости, сложный reconnect, подписанные установщики и релизная сертификация. Не обещать отсутствие утечек на основании поверхностной проверки.

## Регистрация Orca

Run: `run_39546fdb4578`. Статус этапа: реализация macOS разрешена; запуск и результаты исполнителей фиксируются ниже.

- T0: `task_4fc528685e32`; зависимости: нет; Grok.
- T1: `task_8064461b6363`; зависимости: T0; Grok.
- T2: `task_856dec3dc32f`; зависимости: T0; Codex gpt-6-luna low.
- T3: `task_f4c3d7bf80e4`; зависимости: T1, T2; Antigravity.
- T4: `task_e622c3a57fbe`; зависимости: T1, T2; Grok.
- T5: `task_e5ebda514d00`; зависимости: T4; Codex gpt-6-luna low.
- T6: `task_8bb57e07780f`; зависимости: T3, T5; координатор.

## Выполнение macOS MVP

- T0 запущен: Grok, dispatch `ctx_fb9ee4d6722c`, текущий workspace. Отправлено уточнение macOS-only; VPS не является блокером локальной реализации.
- T0 завершён: docs/core-contract.md; mbox commit `1c5b39718ebcd05e6adb31f95246fbf056be9bd7`, локальная arm64 сборка и check приняты. Живой root TUN и VPS-трафик не проверены.
- T1 запущен: Grok, dispatch `ctx_e6460880993b`; только macOS sidecar, без изменения маршрутов хоста.
- T2 запущен параллельно: Codex `gpt-6-luna`, effort `low`, dispatch `ctx_759f3fa6e85a`; область configs/**.
- T6-macOS зарегистрирована: `task_8ba57714e522`; зависит только от T5 (`task_e5ebda514d00`), выполняет координатор. Исходная T6 для всех платформ и T3 остаются отложенными.
- T2 завершён: configs/profile.schema.json, фиктивные примеры и configs/README.md. Исполнитель подтвердил разбор JSON и ограничения полей; координатор просмотрел соответствие полей контракту. Импорт ссылок вне MVP. T4 ожидает завершения T1.
- T1 завершён: core/build-desktop.sh, лицензии, README и sidecar arm64/amd64. Сборка и check прошли; root TUN не запускался.
- T4 запущен: Grok, dispatch `ctx_2c0b7970c9ed`; backend macOS в desktop/src-tauri/** и desktop/README.md.
- Добавлена и запущена T5-prep: task `task_c72d87f845a0`, Antigravity, dispatch `ctx_df3ad8d731e6`; запись только desktop/ui/**. Подготавливает UI параллельно backend.
- Уточнение порядка: T5 Codex запускать только после T4 И T5-prep. В T5 переиспользовать готовый UI, добавить трей и устранить интеграционные несоответствия, не переписывать frontend. Дополнительная зависимость T5-prep контролируется координатором перед запуском (исходный DAG T5 содержит только T4).
- T5-prep завершена: desktop/ui/**, TypeScript/Vite/Tailwind, pnpm build прошёл по отчёту Antigravity. Координатор просмотрел IPC и передал фактические аргументы Grok T4. Native интеграция, трей и root TUN остаются непроверенными. T5 ожидает завершения T4.
- T4 завершён: backend, root-supervisor, профили без возврата пароля; cargo check и 13 тестов прошли. Root TUN не запускался.
- Первая попытка T5 ctx_166b074d3529 завершилась без изменений: исполнитель остановился после обрезанного общего списка файлов. Это ошибка исследования, файлы существуют.
- T5 перезапущена на той же задаче с retry-of: dispatch `ctx_aac64d0b9f5a`, Codex gpt-6-luna low. Переданы точные пути, существующий UI, требования трея, отзывчивого IPC и debug .app.
- Повторная T5 Codex ctx_aac64d0b9f5a завершилась без изменений на чтении файлов. Реального отсутствующего файла не выявлено.
- T5 переназначена Antigravity (автору UI), dispatch `ctx_1b8f0cb5c2ed`, та же задача, retry-of предыдущей попытки; модель Codex не повышалась. Разрешены desktop/ui/**, desktop/src-tauri/** и desktop/README.md для интеграции. Требуются трей, отзывчивые команды, логи и запускаемый debug .app.
- T5 завершена Antigravity: async IPC, UI/logs/tray, debug .app; сборка и 13 backend-тестов прошли по отчёту. Окно и native UI подтверждены исполнителем через Orca computer.
- Координатор при интеграции нашёл runtime blocker: bundled Contents/MacOS/sing-box принят core_config, но supervisor требовал suffixed имя. Назначено исправление Antigravity: task `task_5866b6ac6b18`, dispatch `ctx_54645718105f`; регрессионная проверка, пересборка .app, без root TUN. T6-macOS ожидает это исправление.
- Подготовлены корневой README.md с командами сборки/запуска и ограничениями без VPS, а также .gitignore для генерируемых артефактов.
- Пользователь предоставил временный VPS 31.58.213.192 и разрешил настройку через ssh root@31.58.213.192. Запущена параллельная задача Antigravity `task_99cc8e4d3627`, dispatch `ctx_3a242ac7f9d2`: mita server, docs/VPS_SETUP.md с воспроизводимой инструкцией, приватный профиль вне проекта, реальное проксирование без root TUN. Работа не пересекается с исправлением desktop.
- Исправление bundled sidecar завершено: `task_5866b6ac6b18`. Общая is_allowed_sidecar_name принимает sing-box и dev-имя, отвергает чужие имена; 15 backend-тестов прошли. Debug .app пересобран; check встроенного ядра дал 0, окно повторно подтверждено исполнителем. Root TUN и маршруты не активировались. Следующий шаг — серверная проверка и macOS end-to-end после VPS worker.
- Зарегистрирована финальная сетевая проверка `task_5f596adeef5d`: зависит от исправления .app и готового VPS, выполняет координатор. Сценарий включает сохранение пользовательского профиля, baseline сети, подключение/отключение/выход и восстановление сети.
- VPS задача завершена: mita v3.38.0, 31.58.213.192:28443/TCP, служба RUNNING. Серверный исполнитель подтвердил SOCKS5h и HTTP-проксирование с внешним IP VPS; профиль 0600 вне проекта, docs/VPS_SETUP.md готов.
- Координатор начал macOS E2E: сохранён baseline сети в приватном каталоге вне проекта, установлен временный профиль, GUI загрузил его по IPC. На Mac уже активен utun7 другого VPN — его настройки не менялись. Нажата кнопка подключения; системный SecurityAgent ожидает ввода пользователя. Пароль администратора в чат не запрашивается.
- Пользователь подтвердил системные права. Первая live попытка не подняла TUN: supervisor.log содержит ошибку nohup (cannot detach from console), core.log пустой, root ядро отсутствует; маршрут остался на исходном utun7, прямой IP остался 109.107.176.224. Antigravity назначено исправление фонового запуска из AppleScript, без параллельного TUN.
- Исправление nohup выполняет Antigravity: task `task_b4206afe7e6b`, dispatch `ctx_ce83ea777b3d`. Финальная сетевая проверка ожидает его отчёт.
- Исправлен nohup (`task_b4206afe7e6b`): фон запускается bash trap HUP + exec, stdin /dev/null; AppleScript smoke подтвердил PID/выживание, 15 тестов прошли, .app пересобран. Координатор запустил обновлённый .app, повторный vpn_connect ожидает новый системный диалог прав macOS.
- Вторая live попытка: root supervisor/ядро действительно поднимали utun8 (есть sing-box started в журнале), но GUI не подтвердил root identity и остановил свой туннель. Выявлена причина: proc_pidinfo(PROC_PIDTBSDINFO) для root из обычного GUI возвращает EPERM, хотя proc_pidpath и ps lstart доступны. Исправление cross-UID identity передано Antigravity. AWG utun7 сохранился; собственного TUN/ядра сейчас нет.
- Cross-UID исправление: task `task_9dd89dbf3bb5`, dispatch `ctx_f2e15d8b55ad`, Antigravity; проверка фактического root PID без сигналов, сохранение identity-защит и пересборка .app.
- Cross-UID исправление завершено: 18 тестов, root inspect(1), сравнение времени kernel/sysctl/ps и отрицательные claims прошли; .app пересобран. Старый GUI без ядра остановлен, новый GUI запущен. Orca computer видит окно, но AX чтение нового Mieru отклонено macOS при granted permissions; пользователь попросен вручную нажать Подключить для продолжения сетевой проверки. AWG сохраняется.
- Финальный live E2E прошёл: Mieru utun8, HTTPS без прокси вернул IP VPS 31.58.213.192, example.com HTTP200. Кнопка Отключить завершила root supervisor/ядро, utun8 исчез; AWG utun7 и исходный внешний IP восстановились, HTTPS200. Приложение штатно завершено через AppleEvent quit, своих процессов нет.
- Сетевая задача task_5f596adeef5d отмечена completed. Подробный фактический отчёт и ограничения: docs/MACOS_SMOKE.md. Пункт Выход именно из трея и выход при активном TUN не подтверждены; расширенные проверки не проводились. Пользовательского профиля до теста не было, временный рабочий профиль оставлен приватно для использования до отключения VPS.
- T6-macOS task_8ba57714e522 completed: локальный MVP передан, реальные сетевые результаты и ограничения документированы. Приватный пароль VPS проверен на отсутствие в исходниках/документации. Все процессы Mieru после проверки завершены; сервер оставлен доступным на срок жизни VPS.
- По запросу пользователя продолжены проверки с включённым AWG. Реальные пункты трея подтверждены через mouse event и native menu: Подключить запустил root TUN; Выход при активном TUN завершил GUI/supervisor/ядро и снял utun8. Интернет/AWG utun7 восстановились. Повторное подключение в этом проходе дало timeout HTTP/SOCKS, хотя VPS RUNNING, профиль прежний и часы совпадают. Назначена узкая диагностика Grok без root/смены маршрутов; проверка без AWG отложена до следующего этапа.
- Grok завершил task_cd3b2f0f1dbf: docs/RECONNECT_DIAGNOSTIC.md. Один loopback auto_detect=true воспроизвёл timeout, три свежих true-клиента затем прошли с en1; false и bind utun7 тоже успешны. Устойчивая причина не доказана, код/AWG/сервер не изменены, клиенты остановлены.
- Координатор открыл Mieru для последнего live прохода (повторный TUN, hide/show, аварийное завершение GUI). Mac повторно не предоставляет AX-окно/меню автоматизации при наличии видимого окна; пользователь попросен вручную подключиться. Своих root ядра/TUN до ручного подключения нет, AWG работает.
- Заключительный live проход с AWG завершён успешно: TUN повторно дал IP VPS и HTTPS200; закрытие окна сохранило трафик, Показать окно из трея вернул GUI. Номера интерфейсов в этом проходе динамически поменялись: Mieru utun7, AWG utun8.
- SIGKILL отправлен только точному non-root GUI PID после проверки executable/UID. Supervisor/core самостоятельно вышли за ~0.41с, TUN снят, исходные AWG маршруты/IP и HTTPS200 восстановились. GUI перезапущен: disconnected, active.json очищен, root процессов нет. Базовые MVP-проверки с AWG завершены; необъяснённый разовый timeout остаётся известным наблюдением. Без AWG — следующий отдельный этап.

## Linux 0.1.1 — DMABUF startup (2026-11-13)

Пользователь выбрал отключение DMABUF по умолчанию для всего Linux с сохранением явно заданного окружения. Реализация через Orca Codex `gpt-6-luna`, effort `low`: task_ccaa4bf00267 / ctx_cbb0f0facc9a, продолжение task_c57327ec9602 / ctx_e474428b0a42. Antigravity не использовался. В Linux main() до Tauri добавлен var_os(None) → WEBKIT_DISABLE_DMABUF_RENDERER=1; macOS-ветка не менялась. Версии приложения и Linux defaults согласованы на 0.1.1.

Проверки: cargo check offline/locked0, pnpm build0; Linux release Buildx q1a29z529fh2sda6igbqx517h Completed17m12s. Координатор подтвердил SOURCE_MANIFEST_MATCH_OK и subprocess env cases absent/0/1/empty/non-UTF8. Новый GUI 1dd943a36571dc9f61fd3b023e8baef2869be7e12a982cbdbbffde76231efd9b, helper15226e... без изменений. Xvfb ordinary-user smoke: rendered window, Ctrl+2 profile, настоящий D-BusMenu Quit exit0. NVIDIA/Wayland нового пакета не проверен. Подробности docs/LINUX_DMABUF_FIX.md.

Упаковка 0.1.1 завершена из этого staging без повторной компиляции. Ubuntu/Debian install/remove прошли. Первый Arch упал на download timeout единственного geo mirror; добавлены osbeck/kernel.org/geo fallbacks, Arch-only retry с pacman cache volume прошёл install/remove/ldd. Worker_done succeeded task_c57327ec9602 / ctx_e474428b0a42: verify-packages.sh0 (3 payload hashes/modes/policy и standalone recipe), SHA256SUMS8 entries OK. Модель не повышалась. Пользователь разрешил публикацию GitHub v0.1.1 после проверок; статус prerelease согласно RELEASES.md.
