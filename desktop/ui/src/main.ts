import './style.css';
import type {
  LogEntry,
  MieruProfile,
  ServerHealth,
  SphereVisualState,
  ThemeMode,
  TransportType,
  VpnState,
  VpnStatus,
} from './types';
import {
  checkServerHealth,
  connectVpn,
  disconnectVpn,
  getProfile,
  getVpnLogs,
  getVpnStatus,
  isTauriEnvironment,
  onVpnStatusChange,
  setProfile,
} from './api';
import { MieruSphere } from './sphere';
import { validateProfileFields, validateProfileJson } from './validation';
import { applyTheme, getInitialTheme, setupSystemThemeListener } from './theme';
import { listen } from '@tauri-apps/api/event';

export const APP_NAME = import.meta.env.VITE_APP_NAME?.trim() || 'Avari Mieru';

// ==========================================
// Состояние приложения (в оперативной памяти)
// ==========================================

export type AppTab = 'connection' | 'profile' | 'logs';
export type ProfileMode = 'manual' | 'json';

let activeTab: AppTab = 'connection';
let activeProfileMode: ProfileMode = 'manual';

export function getActiveTab(): AppTab {
  return activeTab;
}

export function getActiveProfileMode(): ProfileMode {
  return activeProfileMode;
}

// Сохранённый профиль (из backend/файла)
let savedProfile: MieruProfile | null = null;
let sessionPassword = ''; // Хранится только в RAM

// Флаг: начал ли пользователь редактировать форму до получения ответа от бэкенда
let userStartedEditingManual = false;
let userStartedEditingJson = false;

// Черновики (Draft state)
let manualDraft: Partial<MieruProfile> = {
  server: '',
  server_port: 8964,
  transport: 'TCP',
  username: '',
  password: '',
  traffic_pattern: '',
};
let isAdvancedOpen = false;
let passwordVisible = false;

let jsonDraftText = '';

let vpnStatus: VpnStatus = {
  state: 'disconnected',
  message: '',
  since: new Date().toISOString(),
};

let lastStatusTimestamp = 0;
const seenCoreLogLines = new Set<string>();
let logPollInterval: number | null = null;

let currentTheme: ThemeMode = getInitialTheme();
const isNative = isTauriEnvironment();

// Procedural 3D Canvas Sphere и Server Health
let sphereInstance: MieruSphere | null = null;
let isColdStartDone = false;
let serverHealth: ServerHealth | null = null;
let isCheckingHealth = false;
let healthCheckGeneration = 0;

let logs: LogEntry[] = [];
let durationInterval: number | null = null;
let toastTimeout: number | null = null;

// ==========================================
// Вспомогательные утилиты очистки и экранирования
// ==========================================

function sanitizeMessage(msg: string): string {
  if (!msg) return '';
  let sanitized = msg.replace(/(password["':=\s]+)[^\s&"',]+/gi, '$1[СКРЫТО]');
  sanitized = sanitized.replace(/(traffic_pattern["':=\s]+)[^\s&"',]+/gi, '$1[СКРЫТО]');
  if (sessionPassword && sessionPassword.length > 2) {
    sanitized = sanitized.split(sessionPassword).join('[СКРЫТО]');
  }
  if (manualDraft.password && manualDraft.password.length > 2) {
    sanitized = sanitized.split(manualDraft.password).join('[СКРЫТО]');
  }
  if (savedProfile?.traffic_pattern && savedProfile.traffic_pattern.length > 2) {
    sanitized = sanitized.split(savedProfile.traffic_pattern).join('[СКРЫТО]');
  }
  return sanitized;
}

function escapeHtml(str: string): string {
  if (!str) return '';
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');
}

function formatDuration(sinceRfc3339: string): string {
  try {
    const start = new Date(sinceRfc3339).getTime();
    if (isNaN(start)) return '00:00:00';
    const now = Date.now();
    const diffSec = Math.max(0, Math.floor((now - start) / 1000));
    const hours = Math.floor(diffSec / 3600);
    const minutes = Math.floor((diffSec % 3600) / 60);
    const seconds = diffSec % 60;
    return [
      hours.toString().padStart(2, '0'),
      minutes.toString().padStart(2, '0'),
      seconds.toString().padStart(2, '0'),
    ].join(':');
  } catch {
    return '00:00:00';
  }
}

// ==========================================
// Логирование и инкрементальный рендеринг
// ==========================================

function addLog(level: LogEntry['level'], rawMessage: string) {
  const message = sanitizeMessage(rawMessage);
  const entry: LogEntry = {
    id: Math.random().toString(36).substring(2, 9),
    timestamp: new Date(),
    level,
    message,
  };
  logs.unshift(entry);
  if (logs.length > 300) {
    logs.pop();
  }
  appendLogEntryToDOM(entry);
  updateLogsCountBadge();
}

function appendLogEntryToDOM(entry: LogEntry) {
  const container = document.getElementById('logs-container');
  if (!container) return;

  const placeholder = container.querySelector('.logs-empty-placeholder');
  if (placeholder) {
    placeholder.remove();
  }

  const el = document.createElement('div');
  el.className =
    'leading-relaxed flex items-start gap-1.5 hover:bg-slate-100/60 dark:hover:bg-slate-900/60 p-0.5 rounded transition-colors animate-log-in';

  const timeStr = entry.timestamp.toLocaleTimeString();
  const colorClasses = {
    info: 'text-slate-600 dark:text-slate-300',
    warn: 'text-amber-600 dark:text-amber-400 font-medium',
    error: 'text-rose-600 dark:text-rose-400 font-semibold',
  }[entry.level];

  const badgeClasses = {
    info: 'text-slate-400',
    warn: 'text-amber-500',
    error: 'text-rose-500',
  }[entry.level];

  el.innerHTML = `
    <span class="text-slate-400 dark:text-slate-600 select-none flex-shrink-0">${timeStr}</span>
    <span class="${badgeClasses} select-none uppercase text-[10px] flex-shrink-0">[${entry.level}]</span>
    <span class="${colorClasses} break-all">${escapeHtml(entry.message)}</span>
  `;

  if (container.firstChild) {
    container.insertBefore(el, container.firstChild);
  } else {
    container.appendChild(el);
  }

  while (container.children.length > 300) {
    container.lastElementChild?.remove();
  }
}

function renderAllLogsFull() {
  const container = document.getElementById('logs-container');
  if (!container) return;

  if (logs.length === 0) {
    container.innerHTML =
      '<div class="logs-empty-placeholder text-slate-400 dark:text-slate-600 text-center py-6 select-none">Журнал событий пуст</div>';
    return;
  }

  container.innerHTML = logs
    .map((log) => {
      const timeStr = log.timestamp.toLocaleTimeString();
      const colorClasses = {
        info: 'text-slate-600 dark:text-slate-300',
        warn: 'text-amber-600 dark:text-amber-400 font-medium',
        error: 'text-rose-600 dark:text-rose-400 font-semibold',
      }[log.level];

      const badgeClasses = {
        info: 'text-slate-400',
        warn: 'text-amber-500',
        error: 'text-rose-500',
      }[log.level];

      return `
      <div class="leading-relaxed flex items-start gap-1.5 hover:bg-slate-100/60 dark:hover:bg-slate-900/60 p-0.5 rounded transition-colors">
        <span class="text-slate-400 dark:text-slate-600 select-none flex-shrink-0">${timeStr}</span>
        <span class="${badgeClasses} select-none uppercase text-[10px] flex-shrink-0">[${log.level}]</span>
        <span class="${colorClasses} break-all">${escapeHtml(log.message)}</span>
      </div>
    `;
    })
    .join('');
}

async function pollCoreLogs() {
  if (!isNative || document.hidden) return;
  try {
    const res = await getVpnLogs();
    if (!res || !res.lines || res.lines.length === 0) return;
    for (const rawLine of res.lines) {
      const line = rawLine.trim();
      if (!line || seenCoreLogLines.has(line)) continue;
      seenCoreLogLines.add(line);
      if (seenCoreLogLines.size > 500) {
        const first = seenCoreLogLines.values().next().value;
        if (first) seenCoreLogLines.delete(first);
      }
      const lower = line.toLowerCase();
      const level: LogEntry['level'] =
        lower.includes('error') || lower.includes('fatal')
          ? 'error'
          : lower.includes('warn')
          ? 'warn'
          : 'info';

      addLog(level, line);
    }
  } catch {
    // Тихо игнорируем ошибку опроса
  }
}

function startLogPolling() {
  if (logPollInterval !== null) return;
  if (isNative && !document.hidden) {
    pollCoreLogs();
  }
  logPollInterval = window.setInterval(() => {
    if (isNative && !document.hidden) {
      pollCoreLogs();
    }
  }, 2000);
}

function stopLogPolling() {
  if (logPollInterval !== null) {
    clearInterval(logPollInterval);
    logPollInterval = null;
  }
}

document.addEventListener('visibilitychange', () => {
  if (document.hidden) {
    stopLogPolling();
  } else {
    startLogPolling();
  }
});

// ==========================================
// Логика визуального состояния сферы и Server Health
// ==========================================

function computeTargetSphereState(): SphereVisualState {
  if (!isColdStartDone) return 'cold_start';
  if (vpnStatus.state === 'connected') {
    if (serverHealth?.state === 'ready') return 'connected';
    if (serverHealth?.state === 'error') return 'error';
    return 'connecting'; // Tunnel active, awaiting server check verification
  }
  if (vpnStatus.state === 'connecting') return 'connecting';
  if (vpnStatus.state === 'disconnecting') return 'disconnecting';
  if (vpnStatus.state === 'error') return 'error';
  // disconnected:
  if (!savedProfile) return 'unconfigured';
  if (serverHealth?.state === 'ready') return 'ready';
  if (serverHealth?.state === 'error') return 'error';
  return 'unconfigured'; // Unknown/busy/unverified is neutral unconfigured
}

function updateSphereVisualState() {
  if (!sphereInstance) return;
  const target = computeTargetSphereState();
  sphereInstance.setState(target);
}

let activeHealthPromise: Promise<void> | null = null;
let queuedHealthCheck: { force: boolean } | null = null;
let lastConnectedHealthCheckTime = 0;

async function runServerHealthCheck(force = false): Promise<void> {
  if (!isNative) {
    serverHealth = {
      state: 'unconfigured',
      latency_ms: null,
      message: 'Проверка доступности недоступна в обычном браузере (запустите через Tauri)',
      checked_at: new Date().toISOString(),
    };
    updateConnectionScreenView();
    updateSphereVisualState();
    return;
  }

  if (!savedProfile) {
    serverHealth = null;
    updateConnectionScreenView();
    updateSphereVisualState();
    return;
  }

  if (activeHealthPromise !== null) {
    queuedHealthCheck = { force: force || (queuedHealthCheck ? queuedHealthCheck.force : false) };
    return activeHealthPromise;
  }

  const currentGen = ++healthCheckGeneration;
  isCheckingHealth = true;
  updateConnectionScreenView();

  activeHealthPromise = (async () => {
    try {
      const health = await checkServerHealth();
      if (currentGen !== healthCheckGeneration) {
        return;
      }

      if (health.state === 'busy') {
        if (serverHealth?.state !== 'ready') {
          addLog('info', 'Проверка сервера: сервер занят, повтор позже');
        }
      } else {
        serverHealth = health;
        const isReady = health.state === 'ready';
        addLog(
          isReady ? 'info' : health.state === 'error' ? 'warn' : 'info',
          `Проверка сервера: ${health.state.toUpperCase()}${
            health.latency_ms != null ? ` (задержка ${health.latency_ms} мс)` : ''
          }${health.message ? ` — ${health.message}` : ''}`
        );
      }
    } catch (err) {
      if (currentGen !== healthCheckGeneration) {
        return;
      }
      const msg = err instanceof Error ? err.message : String(err);
      if (msg.toLowerCase().includes('busy') && serverHealth?.state === 'ready') {
        addLog('info', 'Проверка сервера занята, сохранён прежний статус ready');
      } else {
        serverHealth = {
          state: 'error',
          latency_ms: null,
          message: msg,
          checked_at: new Date().toISOString(),
        };
        addLog('warn', `Ошибка проверки сервера: ${msg}`);
      }
    } finally {
      activeHealthPromise = null;
      if (!queuedHealthCheck) {
        isCheckingHealth = false;
      }
      updateConnectionScreenView();
      updateSphereVisualState();
      if (queuedHealthCheck) {
        const next = queuedHealthCheck;
        queuedHealthCheck = null;
        runServerHealthCheck(next.force);
      }
    }
  })();

  return activeHealthPromise;
}

// ==========================================
// Управление статусом VPN и таймером
// ==========================================

function updateStatus(status: VpnStatus) {
  const statusTime = status.since ? new Date(status.since).getTime() : Date.now();
  if (statusTime < lastStatusTimestamp) {
    return;
  }
  lastStatusTimestamp = statusTime;

  const prev = vpnStatus.state;
  vpnStatus = {
    state: status.state,
    message: sanitizeMessage(status.message || ''),
    since: status.since || new Date().toISOString(),
  };

  if (prev !== status.state) {
    const stateNames: Record<VpnState, string> = {
      disconnected: 'Отключено',
      connecting: 'Подключение...',
      connected: 'Подключено',
      disconnecting: 'Отключение...',
      error: 'Ошибка',
    };
    addLog(
      status.state === 'error' ? 'error' : 'info',
      `Статус VPN: ${stateNames[prev] || prev} → ${stateNames[status.state] || status.state}`
    );
  }

  if (vpnStatus.state === 'connected') {
    startDurationTimer();
    const now = Date.now();
    if ((!serverHealth || serverHealth.state !== 'ready' || now - lastConnectedHealthCheckTime > 30000) && prev !== 'connected') {
      lastConnectedHealthCheckTime = now;
      runServerHealthCheck(false);
    }
  } else {
    stopDurationTimer();
  }

  updateConnectionScreenView();
  updateHeaderStatusBadge();
  updateSphereVisualState();
}

function startDurationTimer() {
  if (durationInterval !== null) return;
  durationInterval = window.setInterval(() => {
    const el = document.getElementById('vpn-duration-timer');
    if (el) {
      el.textContent = formatDuration(vpnStatus.since);
    }
  }, 1000);
}

function stopDurationTimer() {
  if (durationInterval !== null) {
    clearInterval(durationInterval);
    durationInterval = null;
  }
}

// ==========================================
// Уведомления (Позиционированы выше нижней навигации)
// ==========================================

function showNotification(type: 'success' | 'error' | 'warning' | 'info', message: string) {
  const container = document.getElementById('notification-toast');
  if (!container) return;

  if (toastTimeout !== null) {
    clearTimeout(toastTimeout);
    toastTimeout = null;
  }

  const bgColors = {
    success: 'bg-emerald-600 text-white shadow-emerald-500/25',
    error: 'bg-rose-600 text-white shadow-rose-500/25',
    warning: 'bg-amber-500 text-slate-950 shadow-amber-500/25',
    info: 'bg-blue-600 text-white shadow-blue-500/25',
  };

  container.className = `fixed bottom-16 sm:bottom-18 left-1/2 -translate-x-1/2 max-w-[90%] z-50 px-4 py-2 rounded-xl shadow-lg text-xs sm:text-sm font-medium transition-all duration-200 transform translate-y-0 opacity-100 flex items-center gap-2 pointer-events-auto ${bgColors[type]}`;
  container.textContent = sanitizeMessage(message);

  toastTimeout = window.setTimeout(() => {
    container.className =
      'fixed bottom-16 sm:bottom-18 left-1/2 -translate-x-1/2 max-w-[90%] z-50 px-4 py-2 rounded-xl shadow-lg text-xs sm:text-sm font-medium transition-all duration-200 transform translate-y-3 opacity-0 pointer-events-none';
    toastTimeout = null;
  }, 3500);
}

// ==========================================
// Действия: VPN Connect / Disconnect
// ==========================================

async function handleConnectToggle() {
  if (!isNative) {
    showNotification(
      'warning',
      'Нативный бэкенд недоступен в обычном браузере. Запустите настольное приложение Tauri.'
    );
    return;
  }

  if (vpnStatus.state === 'connected') {
    try {
      addLog('info', 'Запрос на отключение VPN...');
      updateStatus({ ...vpnStatus, state: 'disconnecting', message: '' });
      await disconnectVpn();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      addLog('error', `Ошибка при отключении: ${msg}`);
      updateStatus({ ...vpnStatus, state: 'error', message: msg });
    }
  } else if (vpnStatus.state === 'disconnected' || vpnStatus.state === 'error') {
    if (!savedProfile) {
      showNotification('error', 'Перед подключением настройте и сохраните профиль сервера');
      switchTab('profile');
      return;
    }
    try {
      addLog('info', `Запрос на подключение VPN (${savedProfile.server}:${savedProfile.server_port})...`);
      updateStatus({ ...vpnStatus, state: 'connecting', message: '' });
      await connectVpn();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      addLog('error', `Ошибка при подключении: ${msg}`);
      updateStatus({ ...vpnStatus, state: 'error', message: msg });
    }
  }
}

// ==========================================
// Сохранение и импорт профиля
// ==========================================

async function handleSaveManualForm(e: SubmitEvent) {
  e.preventDefault();

  syncManualDraftFromInputs();

  let finalPassword = (manualDraft.password || '').trim();
  if (!finalPassword && sessionPassword) {
    finalPassword = sessionPassword;
  }

  if (!finalPassword) {
    showNotification('error', 'Для сохранения профиля необходимо указать пароль');
    const pwdInput = document.getElementById('input-password');
    pwdInput?.focus();
    return;
  }

  const profileCandidate: Partial<MieruProfile> = {
    server: (manualDraft.server || '').trim(),
    server_port: Number(manualDraft.server_port),
    transport: manualDraft.transport as TransportType,
    username: (manualDraft.username || '').trim(),
    password: finalPassword,
    traffic_pattern: (manualDraft.traffic_pattern || '').trim() || undefined,
  };

  const validation = validateProfileFields(profileCandidate, true);
  if (!validation.valid) {
    const firstErr = Object.values(validation.errors)[0];
    showNotification('error', firstErr);
    return;
  }

  const profileToSave: MieruProfile = profileCandidate as MieruProfile;

  try {
    if (isNative) {
      await setProfile(profileToSave);
      showNotification('success', 'Профиль успешно сохранён в backend');
    } else {
      showNotification('info', 'Проверено в браузере (сохранено временно в памяти)');
    }

    savedProfile = { ...profileToSave };
    sessionPassword = finalPassword;
    ++healthCheckGeneration;
    serverHealth = null;
    isCheckingHealth = false;
    updateConnectionScreenView();
    updateSphereVisualState();
    runServerHealthCheck(true);

    // Нормализуем manualDraft без потери позиции фокуса
    manualDraft = {
      server: profileToSave.server,
      server_port: profileToSave.server_port,
      transport: profileToSave.transport,
      username: profileToSave.username,
      password: finalPassword,
      traffic_pattern: profileToSave.traffic_pattern || '',
    };

    // ВАЖНО: Не затираем пользовательский jsonDraftText если пользователь редактировал JSON
    if (!userStartedEditingJson) {
      jsonDraftText = JSON.stringify(profileToSave, null, 2);
      const jsonTextarea = document.getElementById('textarea-json-import') as HTMLTextAreaElement | null;
      if (jsonTextarea) {
        jsonTextarea.value = jsonDraftText;
      }
    }

    addLog(
      'info',
      `Профиль сохранён: ${profileToSave.server}:${profileToSave.server_port} (${profileToSave.transport})`
    );

    updateProfileViewStatus();
    updateConnectionScreenView();
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    addLog('error', `Не удалось сохранить профиль: ${msg}`);
    showNotification('error', msg);
  }
}

function handleImportJson(jsonText: string) {
  const result = validateProfileJson(jsonText);
  if (!result.valid || !result.profile) {
    showNotification('error', result.error || 'Неверный формат JSON профиля');
    addLog('warn', `Ошибка импорта JSON: ${result.error}`);
    return;
  }

  const profile = result.profile;

  const onProfileReady = (saved: MieruProfile) => {
    savedProfile = { ...saved };
    sessionPassword = saved.password || '';
    ++healthCheckGeneration;
    serverHealth = null;
    isCheckingHealth = false;
    updateConnectionScreenView();
    updateSphereVisualState();
    runServerHealthCheck(true);

    // Синхронизируем jsonDraftText
    jsonDraftText = jsonText;
    const jsonTextarea = document.getElementById('textarea-json-import') as HTMLTextAreaElement | null;
    if (jsonTextarea) {
      jsonTextarea.value = jsonText;
    }

    // Если пользователь не начинал вручную вводить форму — синхронизируем manual draft
    if (!userStartedEditingManual) {
      manualDraft = {
        server: saved.server,
        server_port: saved.server_port,
        transport: saved.transport,
        username: saved.username,
        password: sessionPassword,
        traffic_pattern: saved.traffic_pattern || '',
      };
      populateManualFormValues();
    }

    addLog(
      'info',
      `Импортирован профиль: ${saved.server}:${saved.server_port} (${saved.transport})`
    );

    if (isNative) {
      showNotification('success', 'JSON-профиль проверен и сохранён');
    } else {
      showNotification('info', 'Проверено в браузере (сохранено временно в памяти)');
    }

    updateProfileViewStatus();
    updateConnectionScreenView();
  };

  if (isNative) {
    setProfile(profile)
      .then(() => onProfileReady(profile))
      .catch((err) => {
        const msg = err instanceof Error ? err.message : String(err);
        addLog('error', `Ошибка сохранения импортированного профиля: ${msg}`);
        showNotification('error', msg);
      });
  } else {
    onProfileReady(profile);
  }
}

function loadExampleProfile() {
  const example: MieruProfile = {
    server: '203.0.113.10',
    server_port: 8964,
    transport: 'TCP',
    username: 'baozi',
    password: 'manlianpenfen',
    traffic_pattern: 'GgQIARAK',
  };

  manualDraft = { ...example };
  jsonDraftText = JSON.stringify(example, null, 2);

  populateManualFormValues();
  const jsonTextarea = document.getElementById('textarea-json-import') as HTMLTextAreaElement | null;
  if (jsonTextarea) {
    jsonTextarea.value = jsonDraftText;
  }

  showNotification('info', 'Пример профиля загружен в черновик. Нажмите «Сохранить» для применения.');
  updateProfileViewStatus();
}

function toggleTheme() {
  const next: Record<ThemeMode, ThemeMode> = {
    system: 'light',
    light: 'dark',
    dark: 'system',
  };
  currentTheme = next[currentTheme];
  applyTheme(currentTheme);
  updateThemeButtonUI();
  const isDark =
    currentTheme === 'dark' ||
    (currentTheme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
  sphereInstance?.setDark(isDark);
}

// ==========================================
// Синхронизация полей ввода черновика
// ==========================================

function syncManualDraftFromInputs() {
  const form = document.getElementById('form-profile-manual') as HTMLFormElement | null;
  if (!form) return;
  const formData = new FormData(form);

  manualDraft.server = String(formData.get('server') || '');
  const portVal = Number(formData.get('server_port'));
  manualDraft.server_port = isNaN(portVal) ? 8964 : portVal;
  manualDraft.transport = String(formData.get('transport') || 'TCP') as TransportType;
  manualDraft.username = String(formData.get('username') || '');
  manualDraft.password = String(formData.get('password') || '');
  manualDraft.traffic_pattern = String(formData.get('traffic_pattern') || '');
}

function populateManualFormValues() {
  const inputServer = document.getElementById('input-server') as HTMLInputElement | null;
  const inputPort = document.getElementById('input-port') as HTMLInputElement | null;
  const inputUsername = document.getElementById('input-username') as HTMLInputElement | null;
  const inputPassword = document.getElementById('input-password') as HTMLInputElement | null;
  const inputPattern = document.getElementById('input-traffic-pattern') as HTMLInputElement | null;
  const radioTcp = document.getElementById('radio-transport-tcp') as HTMLInputElement | null;
  const radioUdp = document.getElementById('radio-transport-udp') as HTMLInputElement | null;

  if (inputServer) inputServer.value = manualDraft.server || '';
  if (inputPort) inputPort.value = String(manualDraft.server_port || 8964);
  if (inputUsername) inputUsername.value = manualDraft.username || '';
  if (inputPassword) inputPassword.value = manualDraft.password || sessionPassword || '';
  if (inputPattern) inputPattern.value = manualDraft.traffic_pattern || '';

  if (radioTcp && radioUdp) {
    if (manualDraft.transport === 'UDP') {
      radioUdp.checked = true;
    } else {
      radioTcp.checked = true;
    }
  }
}

function isManualDraftModified(): boolean {
  if (!savedProfile) return true;
  if ((manualDraft.server || '').trim() !== savedProfile.server) return true;
  if (Number(manualDraft.server_port) !== savedProfile.server_port) return true;
  if (manualDraft.transport !== savedProfile.transport) return true;
  if ((manualDraft.username || '').trim() !== savedProfile.username) return true;
  if ((manualDraft.traffic_pattern || '').trim() !== (savedProfile.traffic_pattern || '')) return true;
  if (manualDraft.password && manualDraft.password.trim() !== sessionPassword) return true;
  return false;
}

function isJsonDraftModified(): boolean {
  if (!savedProfile) return true;
  if (!jsonDraftText.trim()) return true; // Пустой JSON считается несохранённым/изменённым
  try {
    const parsed = JSON.parse(jsonDraftText);
    if (parsed.server !== savedProfile.server) return true;
    if (Number(parsed.server_port) !== savedProfile.server_port) return true;
    if (parsed.transport !== savedProfile.transport) return true;
    if (parsed.username !== savedProfile.username) return true;
    if ((parsed.traffic_pattern || '') !== (savedProfile.traffic_pattern || '')) return true;
    if (parsed.password && parsed.password !== sessionPassword) return true;
    return false;
  } catch {
    return true;
  }
}

// ==========================================
// Переключение табов со скользящим индикатором
// ==========================================

function switchTab(newTab: AppTab) {
  if (activeTab === newTab) return;
  activeTab = newTab;

  const tabs: AppTab[] = ['connection', 'profile', 'logs'];
  const tabButtons = tabs.map((t) => document.getElementById(`nav-tab-${t}`));

  tabs.forEach((tab, index) => {
    const btn = tabButtons[index];
    const panel = document.getElementById(`panel-${tab}`);

    if (tab === newTab) {
      btn?.classList.add('text-blue-600', 'dark:text-blue-400');
      btn?.classList.remove('text-slate-600', 'dark:text-slate-400');
      btn?.setAttribute('aria-selected', 'true');
      btn?.setAttribute('tabindex', '0');

      if (panel) {
        panel.classList.remove('is-hidden');
        panel.classList.add('is-active');
        panel.removeAttribute('inert');
      }
    } else {
      btn?.classList.remove('text-blue-600', 'dark:text-blue-400');
      btn?.classList.add('text-slate-600', 'dark:text-slate-400');
      btn?.setAttribute('aria-selected', 'false');
      btn?.setAttribute('tabindex', '-1');

      if (panel) {
        panel.classList.remove('is-active');
        panel.classList.add('is-hidden');
        panel.setAttribute('inert', '');
      }
    }
  });

  updateMainTabSlidingIndicator();

  sphereInstance?.setTabActive(newTab === 'connection');

  if (newTab === 'profile') {
    updateProfileViewStatus();
    updateSubTabSlidingIndicator();
  } else if (newTab === 'logs') {
    const container = document.getElementById('logs-container');
    if (container && container.children.length === 0) {
      renderAllLogsFull();
    }
  }
}

function updateMainTabSlidingIndicator() {
  const indicator = document.getElementById('nav-tab-indicator');
  const activeBtn = document.getElementById(`nav-tab-${activeTab}`);
  const track = document.getElementById('nav-tab-track');
  if (!indicator || !activeBtn || !track) return;

  const trackRect = track.getBoundingClientRect();
  const btnRect = activeBtn.getBoundingClientRect();

  const left = btnRect.left - trackRect.left;
  const width = btnRect.width;

  indicator.style.transform = `translateX(${left}px)`;
  indicator.style.width = `${width}px`;
}

function switchProfileSubTab(newMode: ProfileMode) {
  if (activeProfileMode === newMode) return;
  activeProfileMode = newMode;
  const btnManual = document.getElementById('subtab-manual');
  const btnJson = document.getElementById('subtab-json');
  const viewManual = document.getElementById('profile-view-manual');
  const viewJson = document.getElementById('profile-view-json');

  if (newMode === 'manual') {
    btnManual?.classList.add('text-slate-900', 'dark:text-white');
    btnManual?.classList.remove('text-slate-600', 'dark:text-slate-400');
    btnManual?.setAttribute('aria-selected', 'true');
    btnManual?.setAttribute('tabindex', '0');

    btnJson?.classList.remove('text-slate-900', 'dark:text-white');
    btnJson?.classList.add('text-slate-600', 'dark:text-slate-400');
    btnJson?.setAttribute('aria-selected', 'false');
    btnJson?.setAttribute('tabindex', '-1');

    if (viewManual) {
      viewManual.classList.remove('is-hidden');
      viewManual.classList.add('is-active');
      viewManual.removeAttribute('inert');
    }
    if (viewJson) {
      viewJson.classList.remove('is-active');
      viewJson.classList.add('is-hidden');
      viewJson.setAttribute('inert', '');
    }
  } else {
    btnJson?.classList.add('text-slate-900', 'dark:text-white');
    btnJson?.classList.remove('text-slate-600', 'dark:text-slate-400');
    btnJson?.setAttribute('aria-selected', 'true');
    btnJson?.setAttribute('tabindex', '0');

    btnManual?.classList.remove('text-slate-900', 'dark:text-white');
    btnManual?.classList.add('text-slate-600', 'dark:text-slate-400');
    btnManual?.setAttribute('aria-selected', 'false');
    btnManual?.setAttribute('tabindex', '-1');

    if (viewJson) {
      viewJson.classList.remove('is-hidden');
      viewJson.classList.add('is-active');
      viewJson.removeAttribute('inert');
    }
    if (viewManual) {
      viewManual.classList.remove('is-active');
      viewManual.classList.add('is-hidden');
      viewManual.setAttribute('inert', '');
    }

    // ВАЖНО: Если пользователь стёр JSON (он пустой) или начал редактировать, НЕ восстанавливаем savedProfile!
    // Восстанавливаем сохранённый только если jsonDraftText никогда не устанавливался и пользователь не редактировал
    const textarea = document.getElementById('textarea-json-import') as HTMLTextAreaElement | null;
    if (textarea && !userStartedEditingJson && !jsonDraftText && savedProfile) {
      jsonDraftText = JSON.stringify(savedProfile, null, 2);
      textarea.value = jsonDraftText;
    }
  }

  updateSubTabSlidingIndicator();
  updateProfileViewStatus();
}

function updateSubTabSlidingIndicator() {
  const indicator = document.getElementById('subtab-indicator');
  const activeBtn = document.getElementById(activeProfileMode === 'manual' ? 'subtab-manual' : 'subtab-json');
  const track = document.getElementById('subtab-track');
  if (!indicator || !activeBtn || !track) return;

  const trackRect = track.getBoundingClientRect();
  const btnRect = activeBtn.getBoundingClientRect();

  const left = btnRect.left - trackRect.left;
  const width = btnRect.width;

  indicator.style.transform = `translateX(${left}px)`;
  indicator.style.width = `${width}px`;
}

// ==========================================
// Динамическое обновление представлений
// ==========================================

function updateHeaderStatusBadge() {
  const container = document.getElementById('header-status-badge');
  if (!container) return;
  container.innerHTML = renderStateBadge(vpnStatus.state);
}

function updateThemeButtonUI() {
  const btn = document.getElementById('btn-toggle-theme');
  if (!btn) return;
  btn.innerHTML = renderThemeIcon(currentTheme);
  btn.setAttribute('title', `Переключить тему оформления (Текущая: ${currentTheme})`);
}

function updateLogsCountBadge() {
  const badge = document.getElementById('nav-logs-badge');
  if (!badge) return;
  if (logs.length > 0) {
    badge.textContent = String(logs.length > 99 ? '99+' : logs.length);
    badge.classList.remove('hidden');
  } else {
    badge.classList.add('hidden');
  }
}

function updateConnectionScreenView() {
  const statusTitle = document.getElementById('conn-status-title');
  const statusDesc = document.getElementById('conn-status-desc');
  const serverPill = document.getElementById('conn-server-pill');
  const healthPill = document.getElementById('conn-health-pill');
  const btnConnect = document.getElementById('btn-connect-toggle') as HTMLButtonElement | null;
  const timerContainer = document.getElementById('conn-timer-container');
  const errBox = document.getElementById('conn-error-box');

  if (statusTitle) {
    statusTitle.textContent = renderStatusTitle(vpnStatus.state);
    statusTitle.dataset.tone = getStatusMessageTone();
  }

  if (statusDesc) {
    statusDesc.textContent = renderStatusSubtitle(vpnStatus.state, vpnStatus.message);
    statusDesc.dataset.tone = getStatusMessageTone();
  }

  if (serverPill) {
    if (savedProfile) {
      serverPill.innerHTML = `
        <span class="inline-flex items-center gap-1.5 px-3 py-1 rounded-full bg-slate-100 dark:bg-slate-800 text-xs text-slate-700 dark:text-slate-300 font-medium">
          <span class="w-1.5 h-1.5 rounded-full ${vpnStatus.state === 'connected' ? 'bg-emerald-500' : 'bg-slate-400'}"></span>
          <span>${escapeHtml(savedProfile.server)}:${savedProfile.server_port}</span>
          <span class="font-bold text-[10px] uppercase text-blue-600 dark:text-blue-400 ml-0.5">${escapeHtml(savedProfile.transport)}</span>
        </span>
      `;
    } else {
      serverPill.innerHTML = `
        <button type="button" id="btn-conn-configure-profile" class="inline-flex items-center gap-1.5 px-3 py-1 rounded-full bg-amber-50 dark:bg-amber-950/50 border border-amber-200 dark:border-amber-800/60 text-xs text-amber-700 dark:text-amber-300 font-medium hover:bg-amber-100 transition-colors">
          <span>⚠️ Профиль не настроен — нажмите для настройки</span>
        </button>
      `;
      document.getElementById('btn-conn-configure-profile')?.addEventListener('click', () => switchTab('profile'));
    }
  }

  if (healthPill) {
    if (!savedProfile) {
      healthPill.innerHTML = '';
      healthPill.classList.add('hidden');
    } else {
      healthPill.classList.remove('hidden');
      if (!isNative) {
        healthPill.innerHTML = `
          <span class="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full bg-slate-100 dark:bg-slate-800 text-[11px] text-slate-500 dark:text-slate-400 font-medium select-none">
            <span class="w-1.5 h-1.5 rounded-full bg-slate-400"></span>
            <span>Проверка недоступна в браузере</span>
          </span>
        `;
      } else if (isCheckingHealth) {
        healthPill.innerHTML = `
          <span class="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full bg-blue-50 dark:bg-blue-950/40 border border-blue-200/60 dark:border-blue-800/60 text-[11px] text-blue-700 dark:text-blue-300 font-medium select-none animate-pulse">
            <svg class="w-3 h-3 animate-spin text-blue-600 dark:text-blue-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"/></svg>
            <span>Проверка доступности сервера...</span>
          </span>
        `;
      } else if (serverHealth?.state === 'ready') {
        const latText = serverHealth.latency_ms != null ? ` (${serverHealth.latency_ms} мс)` : '';
        healthPill.innerHTML = `
          <div class="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full bg-emerald-50 dark:bg-emerald-950/40 border border-emerald-200/60 dark:border-emerald-800/60 text-[11px] text-emerald-700 dark:text-emerald-300 font-medium">
            <span class="w-1.5 h-1.5 rounded-full bg-emerald-500"></span>
            <span>Сервер доступен${latText}</span>
            <button type="button" id="btn-health-refresh" title="Повторить проверку" class="p-0.5 rounded text-emerald-600 dark:text-emerald-400 hover:bg-emerald-200/50 dark:hover:bg-emerald-800/50 transition-colors ml-0.5 focus:outline-none focus:ring-1 focus:ring-emerald-500" aria-label="Повторить проверку сервера">
              <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"/></svg>
            </button>
          </div>
        `;
        document.getElementById('btn-health-refresh')?.addEventListener('click', () => runServerHealthCheck(true));
      } else if (serverHealth?.state === 'error') {
        healthPill.innerHTML = `
          <div class="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full bg-amber-50 dark:bg-amber-950/40 border border-amber-200/60 dark:border-amber-800/60 text-[11px] text-amber-700 dark:text-amber-300 font-medium" title="${escapeHtml(serverHealth.message)}">
            <span class="w-1.5 h-1.5 rounded-full bg-amber-500"></span>
            <span class="truncate max-w-[150px]">Сервер недоступен</span>
            <button type="button" id="btn-health-retry" class="px-1.5 py-0.2 rounded bg-amber-200/70 dark:bg-amber-900/60 hover:bg-amber-300/80 text-[10px] text-amber-900 dark:text-amber-200 font-semibold transition-colors ml-0.5 focus:outline-none focus:ring-1 focus:ring-amber-500" aria-label="Повторить проверку">Повторить</button>
          </div>
        `;
        document.getElementById('btn-health-retry')?.addEventListener('click', () => runServerHealthCheck(true));
      } else {
        healthPill.innerHTML = `
          <div class="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full bg-slate-100 dark:bg-slate-800 text-[11px] text-slate-600 dark:text-slate-400 font-medium">
            <span class="w-1.5 h-1.5 rounded-full bg-slate-400"></span>
            <span>Сервер не проверен</span>
            <button type="button" id="btn-health-check" class="px-1.5 py-0.2 rounded bg-slate-200 dark:bg-slate-700 hover:bg-slate-300 text-[10px] text-slate-800 dark:text-slate-200 font-medium transition-colors ml-0.5">Проверить</button>
          </div>
        `;
        document.getElementById('btn-health-check')?.addEventListener('click', () => runServerHealthCheck(true));
      }
    }
  }

  if (timerContainer) {
    if (vpnStatus.state === 'connected' && !renderStatusSubtitle(vpnStatus.state, vpnStatus.message)) {
      timerContainer.classList.remove('hidden');
      timerContainer.innerHTML = `
        <div class="inline-flex items-center gap-1.5 px-3 py-1 rounded-lg bg-emerald-50 dark:bg-emerald-950/40 border border-emerald-200 dark:border-emerald-800/50 text-xs font-mono text-emerald-700 dark:text-emerald-300">
          <span class="w-2 h-2 rounded-full bg-emerald-500"></span>
          <span id="vpn-duration-timer">${formatDuration(vpnStatus.since)}</span>
        </div>
      `;
    } else {
      timerContainer.classList.add('hidden');
      timerContainer.innerHTML = '';
    }
  }

  if (errBox) {
    if (vpnStatus.state === 'error' && vpnStatus.message) {
      errBox.classList.remove('hidden');
      errBox.innerHTML = `
        <div class="p-3 rounded-xl bg-rose-50 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-900/50 text-rose-800 dark:text-rose-300 text-xs flex items-start gap-2">
          <svg class="w-4 h-4 flex-shrink-0 text-rose-600 dark:text-rose-400 mt-0.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/></svg>
          <div class="flex-1 min-w-0">
            <span class="font-semibold">Ошибка подключения:</span>
            <div class="font-mono mt-0.5 break-all text-[11px] leading-tight">${escapeHtml(vpnStatus.message)}</div>
          </div>
        </div>
      `;
    } else {
      errBox.classList.add('hidden');
      errBox.innerHTML = '';
    }
  }

  if (btnConnect) {
    const isPending = vpnStatus.state === 'connecting' || vpnStatus.state === 'disconnecting';
    btnConnect.disabled = isPending;
    btnConnect.className = getConnectButtonClasses(vpnStatus.state);
    btnConnect.innerHTML = renderConnectButtonContent(vpnStatus.state);
  }
}

function updateProfileViewStatus() {
  const badgeContainer = document.getElementById('profile-status-indicator');
  if (!badgeContainer) return;

  const modified = activeProfileMode === 'manual' ? isManualDraftModified() : isJsonDraftModified();

  if (!savedProfile) {
    badgeContainer.innerHTML = `
      <span class="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-[11px] font-medium bg-amber-100 dark:bg-amber-950/60 text-amber-800 dark:text-amber-300">
        <span class="w-1.5 h-1.5 rounded-full bg-amber-500"></span>
        Не сохранён
      </span>
    `;
  } else if (modified) {
    badgeContainer.innerHTML = `
      <span class="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-[11px] font-medium bg-blue-100 dark:bg-blue-950/60 text-blue-800 dark:text-blue-300">
        <span class="w-1.5 h-1.5 rounded-full bg-blue-500"></span>
        Черновик изменён
      </span>
    `;
  } else {
    badgeContainer.innerHTML = `
      <span class="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-[11px] font-medium bg-emerald-100 dark:bg-emerald-950/60 text-emerald-800 dark:text-emerald-300">
        <span class="w-1.5 h-1.5 rounded-full bg-emerald-500"></span>
        ${isNative ? 'Сохранён в backend' : 'Проверено в браузере'}
      </span>
    `;
  }
}

// ==========================================
// Рендеринг основного каркаса (DOM Layout с Grid Stack)
// ==========================================

function renderInitialLayout() {
  const app = document.getElementById('app');
  if (!app) return;

  app.innerHTML = `
    <div id="app-root" class="h-full w-full flex-1 flex flex-col min-h-0 bg-slate-100 dark:bg-slate-950 text-slate-800 dark:text-slate-100 font-sans transition-colors duration-150 overflow-hidden select-none">
      
      <!-- 1. ВЕРХНИЙ ХЕДЕР -->
      <header class="app-header h-16 flex-shrink-0 border-b border-slate-200/80 dark:border-slate-800/80 bg-white/90 dark:bg-slate-900/90 backdrop-blur px-4 flex items-center justify-between z-30">
        <div class="flex items-center gap-2.5 min-w-0">
          <img src="/mieru-logo.png" alt="" class="w-9 h-9 rounded-xl object-contain flex-shrink-0" />
          <div class="min-w-0">
            <span class="font-bold text-base tracking-tight truncate">${escapeHtml(APP_NAME)}</span>
          </div>
        </div>

        <div class="flex items-center gap-1.5">
          <div id="header-status-badge">
            ${renderStateBadge(vpnStatus.state)}
          </div>

          <button
            type="button"
            id="btn-toggle-theme"
            aria-label="Переключить тему оформления"
            class="p-1.5 rounded-lg border border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-slate-800/70 hover:bg-slate-100 dark:hover:bg-slate-700 text-slate-600 dark:text-slate-300 transition-colors focus:outline-none focus:ring-2 focus:ring-blue-500"
            title="Переключить тему оформления"
          >
            ${renderThemeIcon(currentTheme)}
          </button>
        </div>
      </header>

      <!-- 2. БАННЕР БРАУЗЕРНОГО РЕЖИМА -->
      ${!isNative ? `
        <div class="flex-shrink-0 bg-amber-500/10 border-b border-amber-500/25 px-3 py-1 text-xs text-amber-800 dark:text-amber-200 flex items-center justify-between gap-2">
          <div class="flex items-center gap-1.5 min-w-0">
            <svg class="w-4 h-4 flex-shrink-0 text-amber-600 dark:text-amber-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"/></svg>
            <span class="truncate">Режим браузера: форма и валидация доступны, прямое подключение VPN требует запуска через Tauri.</span>
          </div>
        </div>
      ` : ''}

      <!-- 3. ОСНОВНОЙ КОНТЕНТ (GRID STACK OVERLAY ДЛЯ 100% ВЫСОТЫ КАЖДОГО ЭКРАНА) -->
      <main class="flex-1 min-h-0 overflow-hidden relative p-3 sm:p-4 max-w-3xl w-full mx-auto">
        <div class="grid-stack-container">
          
          <!-- ================= PANEL 1: ПОДКЛЮЧЕНИЕ ================= -->
          <section
            id="panel-connection"
            class="grid-stack-item screen-panel is-active flex flex-col justify-between overflow-y-auto"
            role="tabpanel"
            aria-labelledby="nav-tab-connection"
          >
            <div id="connection-overview" class="flex-1 flex flex-col items-center p-2 text-center max-w-sm mx-auto w-full">
              
              <div id="conn-sphere-container" class="relative w-full h-44 sm:h-48 mx-auto flex items-center justify-center mb-1">
                <canvas id="sphere-canvas" width="220" height="220" class="w-full h-full block touch-none select-none" aria-label="3D Status Sphere"></canvas>
              </div>

              <h2 id="conn-status-title" data-tone="${getStatusMessageTone()}" aria-live="polite" class="text-xl sm:text-2xl font-bold tracking-tight mb-1">
                ${renderStatusTitle(vpnStatus.state)}
              </h2>

              <div class="connection-context">
              <div id="conn-timer-container" class="${vpnStatus.state === 'connected' && !renderStatusSubtitle(vpnStatus.state, vpnStatus.message) ? '' : 'hidden'}">
                ${vpnStatus.state === 'connected' ? `
                  <div class="inline-flex items-center gap-1.5 px-3 py-1 rounded-lg bg-emerald-50 dark:bg-emerald-950/40 border border-emerald-200 dark:border-emerald-800/50 text-xs font-mono text-emerald-700 dark:text-emerald-300">
                    <span class="w-2 h-2 rounded-full bg-emerald-500"></span>
                    <span id="vpn-duration-timer">${formatDuration(vpnStatus.since)}</span>
                  </div>
                ` : ''}
              </div>

              <p id="conn-status-desc" data-tone="${getStatusMessageTone()}" role="status" aria-live="polite">
                ${renderStatusSubtitle(vpnStatus.state, vpnStatus.message)}
              </p>
              </div>

              <div id="conn-server-pill" class="mb-1.5">
                ${savedProfile ? `
                  <span class="inline-flex items-center gap-1.5 px-3 py-1 rounded-full bg-slate-200/70 dark:bg-slate-800/80 text-xs text-slate-700 dark:text-slate-300 font-medium">
                    <span class="w-1.5 h-1.5 rounded-full ${vpnStatus.state === 'connected' ? 'bg-emerald-500' : 'bg-slate-400'}"></span>
                    <span>${escapeHtml(savedProfile.server)}:${savedProfile.server_port}</span>
                    <span class="font-bold text-[10px] uppercase text-blue-600 dark:text-blue-400 ml-0.5">${escapeHtml(savedProfile.transport)}</span>
                  </span>
                ` : `
                  <button type="button" id="btn-conn-configure-profile" class="inline-flex items-center gap-1.5 px-3 py-1 rounded-full bg-amber-50 dark:bg-amber-950/50 border border-amber-200 dark:border-amber-800/60 text-xs text-amber-700 dark:text-amber-300 font-medium hover:bg-amber-100 transition-colors">
                    <span>⚠️ Профиль не настроен — нажмите для настройки</span>
                  </button>
                `}
              </div>

              <div id="conn-health-pill" class="mb-2"></div>

              <div id="conn-error-box" class="w-full mb-2 ${vpnStatus.state === 'error' && vpnStatus.message ? '' : 'hidden'}">
                ${vpnStatus.state === 'error' && vpnStatus.message ? `
                  <div class="p-3 rounded-xl bg-rose-50 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-900/50 text-rose-800 dark:text-rose-300 text-xs flex items-start gap-2">
                    <svg class="w-4 h-4 flex-shrink-0 text-rose-600 dark:text-rose-400 mt-0.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/></svg>
                    <div class="flex-1 min-w-0">
                      <span class="font-semibold">Ошибка подключения:</span>
                      <div class="font-mono mt-0.5 break-all text-[11px] leading-tight">${escapeHtml(vpnStatus.message)}</div>
                    </div>
                  </div>
                ` : ''}
              </div>

            </div>

            <div class="p-2 w-full max-w-sm mx-auto flex-shrink-0">
              <button
                type="button"
                id="btn-connect-toggle"
                class="${getConnectButtonClasses(vpnStatus.state)}"
                ${vpnStatus.state === 'connecting' || vpnStatus.state === 'disconnecting' ? 'disabled' : ''}
              >
                ${renderConnectButtonContent(vpnStatus.state)}
              </button>
            </div>
          </section>

          <!-- ================= PANEL 2: ПРОФИЛЬ ================= -->
          <section
            id="panel-profile"
            class="grid-stack-item screen-panel is-hidden flex flex-col overflow-y-auto"
            role="tabpanel"
            aria-labelledby="nav-tab-profile"
            inert
          >
            <div class="bg-white dark:bg-slate-900 rounded-2xl p-4 sm:p-5 shadow-xs border border-slate-200/80 dark:border-slate-800/80 flex-1 flex flex-col min-h-0">
              
              <!-- Переключатель подвкладок -->
              <div class="flex items-center justify-between border-b border-slate-100 dark:border-slate-800 pb-2.5 mb-2.5 gap-2 flex-shrink-0">
                <div id="subtab-track" class="relative flex items-center rounded-lg bg-slate-100 dark:bg-slate-800/80 p-0.5" role="tablist" aria-label="Формат ввода профиля">
                  <div id="subtab-indicator" class="subtab-indicator"></div>
                  <button
                    type="button"
                    id="subtab-manual"
                    role="tab"
                    aria-selected="true"
                    aria-controls="profile-view-manual"
                    tabindex="0"
                    class="relative z-10 px-2.5 py-1 text-xs font-medium rounded-md transition-colors text-slate-900 dark:text-white"
                  >
                    Ручной ввод
                  </button>
                  <button
                    type="button"
                    id="subtab-json"
                    role="tab"
                    aria-selected="false"
                    aria-controls="profile-view-json"
                    tabindex="-1"
                    class="relative z-10 px-2.5 py-1 text-xs font-medium rounded-md transition-colors text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-white"
                  >
                    JSON конфигурация
                  </button>
                </div>

                <div id="profile-status-indicator">
                  <!-- Заполняется updateProfileViewStatus() -->
                </div>
              </div>

              <!-- Контейнер для Manual и JSON в виде Grid Stack (наложение) -->
              <div class="grid-stack-container flex-1 min-h-0">
                
                <!-- Подпанель 1: Ручной ввод -->
                <div id="profile-view-manual" role="tabpanel" aria-labelledby="subtab-manual" class="grid-stack-item screen-panel is-active flex flex-col overflow-y-auto">
                  <form id="form-profile-manual" class="space-y-3 flex-1 flex flex-col justify-between">
                    <div class="space-y-2.5">
                      
                      <!-- Сервер и Порт -->
                      <div class="grid grid-cols-3 gap-2">
                        <div class="col-span-2">
                          <label for="input-server" class="block text-[11px] font-semibold text-slate-700 dark:text-slate-300 mb-0.5">
                            Сервер (IP или домен) <span class="text-rose-500">*</span>
                          </label>
                          <input
                            type="text"
                            id="input-server"
                            name="server"
                            required
                            placeholder="203.0.113.10"
                            class="w-full px-2.5 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800/70 focus:bg-white dark:focus:bg-slate-800 focus:outline-none focus:ring-2 focus:ring-blue-500 text-xs font-mono"
                          />
                        </div>
                        <div>
                          <label for="input-port" class="block text-[11px] font-semibold text-slate-700 dark:text-slate-300 mb-0.5">
                            Порт <span class="text-rose-500">*</span>
                          </label>
                          <input
                            type="number"
                            id="input-port"
                            name="server_port"
                            required
                            min="1"
                            max="65535"
                            placeholder="8964"
                            class="w-full px-2.5 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800/70 focus:bg-white dark:focus:bg-slate-800 focus:outline-none focus:ring-2 focus:ring-blue-500 text-xs font-mono"
                          />
                        </div>
                      </div>

                      <!-- Транспорт и Логин -->
                      <div class="grid grid-cols-2 gap-2">
                        <div>
                          <span id="label-transport-group" class="block text-[11px] font-semibold text-slate-700 dark:text-slate-300 mb-0.5">
                            Транспорт <span class="text-rose-500">*</span>
                          </span>
                          <div class="grid grid-cols-2 gap-1.5" role="radiogroup" aria-labelledby="label-transport-group">
                            <label class="transport-radio-label cursor-pointer border border-slate-300 dark:border-slate-700 rounded-lg py-1.5 text-center text-xs font-medium text-slate-700 dark:text-slate-300 has-[:checked]:bg-blue-600 has-[:checked]:text-white has-[:checked]:border-blue-600 transition-colors">
                              <input type="radio" id="radio-transport-tcp" name="transport" value="TCP" class="sr-only" checked />
                              <span>TCP</span>
                            </label>
                            <label class="transport-radio-label cursor-pointer border border-slate-300 dark:border-slate-700 rounded-lg py-1.5 text-center text-xs font-medium text-slate-700 dark:text-slate-300 has-[:checked]:bg-blue-600 has-[:checked]:text-white has-[:checked]:border-blue-600 transition-colors">
                              <input type="radio" id="radio-transport-udp" name="transport" value="UDP" class="sr-only" />
                              <span>UDP</span>
                            </label>
                          </div>
                        </div>

                        <div>
                          <label for="input-username" class="block text-[11px] font-semibold text-slate-700 dark:text-slate-300 mb-0.5">
                            Имя пользователя <span class="text-rose-500">*</span>
                          </label>
                          <input
                            type="text"
                            id="input-username"
                            name="username"
                            required
                            placeholder="username"
                            class="w-full px-2.5 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800/70 focus:bg-white dark:focus:bg-slate-800 focus:outline-none focus:ring-2 focus:ring-blue-500 text-xs"
                          />
                        </div>
                      </div>

                      <!-- Пароль -->
                      <div>
                        <div class="flex items-center justify-between mb-0.5">
                          <label for="input-password" class="block text-[11px] font-semibold text-slate-700 dark:text-slate-300">
                            Пароль <span class="text-rose-500">*</span>
                          </label>
                          <span class="text-[10px] text-slate-400 dark:text-slate-500">Только в оперативной памяти</span>
                        </div>
                        <div class="relative">
                          <input
                            type="password"
                            id="input-password"
                            name="password"
                            placeholder="Пароль профиля"
                            class="w-full pl-2.5 pr-8 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800/70 focus:bg-white dark:focus:bg-slate-800 focus:outline-none focus:ring-2 focus:ring-blue-500 text-xs font-mono"
                          />
                          <button
                            type="button"
                            id="btn-toggle-password-visibility"
                            aria-label="Показать или скрыть пароль"
                            class="absolute right-1.5 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-600 dark:hover:text-slate-300 p-1"
                          >
                            <svg id="icon-pwd-show" class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z"/></svg>
                            <svg id="icon-pwd-hide" class="w-3.5 h-3.5 hidden" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l18 18"/></svg>
                          </button>
                        </div>
                      </div>

                      <!-- Дополнительные параметры (с инертным закрытием) -->
                      <div class="border-t border-slate-100 dark:border-slate-800 pt-2">
                        <button
                          type="button"
                          id="btn-toggle-advanced"
                          aria-expanded="false"
                          aria-controls="box-advanced"
                          class="text-xs text-slate-500 dark:text-slate-400 hover:text-slate-800 dark:hover:text-slate-200 flex items-center gap-1 font-medium transition-colors"
                        >
                          <svg id="icon-arrow-advanced" class="w-3.5 h-3.5 transition-transform" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7"/></svg>
                          <span>Дополнительные параметры</span>
                        </button>

                        <div id="box-advanced" class="collapsible-content is-closed mt-2" inert>
                          <label for="input-traffic-pattern" class="block text-[11px] font-semibold text-slate-700 dark:text-slate-300 mb-0.5">
                            Шаблон маскировки трафика (Base64)
                          </label>
                          <input
                            type="text"
                            id="input-traffic-pattern"
                            name="traffic_pattern"
                            placeholder="Например: GgQIARAK"
                            class="w-full px-2.5 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800/70 focus:bg-white dark:focus:bg-slate-800 focus:outline-none focus:ring-2 focus:ring-blue-500 text-xs font-mono"
                          />
                          <p class="text-[10px] text-slate-400 dark:text-slate-500 mt-0.5">Base64 от Protobuf TrafficPattern</p>
                        </div>
                      </div>

                    </div>

                    <!-- Кнопки управления формой -->
                    <div class="flex items-center justify-between pt-2.5 border-t border-slate-100 dark:border-slate-800 gap-2 flex-shrink-0">
                      <button
                        type="button"
                        id="btn-load-example"
                        class="text-xs text-blue-600 dark:text-blue-400 hover:underline font-medium py-1"
                      >
                        Заполнить пример
                      </button>

                      <button
                        type="submit"
                        class="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-700 text-white font-semibold text-xs shadow-xs transition-colors"
                      >
                        Сохранить профиль
                      </button>
                    </div>
                  </form>
                </div>

                <!-- Подпанель 2: Импорт JSON -->
                <div id="profile-view-json" role="tabpanel" aria-labelledby="subtab-json" class="grid-stack-item screen-panel is-hidden flex flex-col justify-between space-y-2.5" inert>
                  <div class="flex-1 flex flex-col min-h-0">
                    <div class="flex items-center justify-between mb-1 flex-shrink-0">
                      <label for="textarea-json-import" class="block text-[11px] font-semibold text-slate-700 dark:text-slate-300">
                        JSON профиля
                      </label>
                      <button
                        type="button"
                        id="btn-paste-example-json"
                        class="text-xs text-blue-600 dark:text-blue-400 hover:underline"
                      >
                        Вставить пример
                      </button>
                    </div>

                    <textarea
                      id="textarea-json-import"
                      class="w-full flex-1 min-h-[140px] p-2.5 rounded-xl border border-slate-300 dark:border-slate-700 bg-slate-50 dark:bg-slate-800/70 focus:bg-white dark:focus:bg-slate-800 focus:outline-none focus:ring-2 focus:ring-blue-500 text-xs font-mono resize-none"
                      placeholder='{\n  "server": "203.0.113.10",\n  "server_port": 8964,\n  "transport": "TCP",\n  "username": "baozi",\n  "password": "manlianpenfen"\n}'
                    ></textarea>
                  </div>

                  <div class="flex items-center justify-between pt-2 border-t border-slate-100 dark:border-slate-800 gap-2 flex-shrink-0">
                    <input type="file" id="input-file-json" accept=".json,application/json" class="hidden" />
                    <button
                      type="button"
                      id="btn-pick-file-json"
                      class="px-3 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 hover:bg-slate-50 dark:hover:bg-slate-800 text-xs font-medium text-slate-700 dark:text-slate-300 transition-colors"
                    >
                      Выбрать .json файл...
                    </button>

                    <button
                      type="button"
                      id="btn-submit-json"
                      class="px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-700 text-white font-semibold text-xs shadow-xs transition-colors"
                    >
                      Применить JSON
                    </button>
                  </div>
                </div>

              </div>

            </div>
          </section>

          <!-- ================= PANEL 3: ЖУРНАЛ ================= -->
          <section
            id="panel-logs"
            class="grid-stack-item screen-panel is-hidden flex flex-col overflow-hidden"
            role="tabpanel"
            aria-labelledby="nav-tab-logs"
            inert
          >
            <div class="bg-white dark:bg-slate-900 rounded-2xl p-3 sm:p-4 shadow-xs border border-slate-200/80 dark:border-slate-800/80 flex-1 flex flex-col overflow-hidden min-h-0">
              <div class="flex items-center justify-between pb-2 mb-2 border-b border-slate-100 dark:border-slate-800 flex-shrink-0">
                <div class="flex items-center gap-2">
                  <h3 class="text-xs font-bold uppercase tracking-wider text-slate-500 dark:text-slate-400">События и журнал</h3>
                  <span class="text-[10px] text-slate-400 dark:text-slate-500">(пароли скрыты)</span>
                </div>
                <button
                  type="button"
                  id="btn-clear-logs"
                  class="text-xs px-2.5 py-1 rounded-md border border-slate-200 dark:border-slate-800 hover:bg-slate-50 dark:hover:bg-slate-800 text-slate-500 dark:text-slate-400 transition-colors"
                >
                  Очистить
                </button>
              </div>

              <div
                id="logs-container"
                tabindex="0"
                role="region"
                aria-label="Логи приложения"
                class="flex-1 overflow-y-auto space-y-1 text-[11px] font-mono rounded-xl bg-slate-50 dark:bg-slate-950/80 p-2.5 border border-slate-100 dark:border-slate-800/70"
              >
                <!-- Заполняется инкрементально -->
              </div>
            </div>
          </section>

        </div>
      </main>

      <!-- 4. ПОСТОЯННАЯ НАВИГАЦИОННАЯ ПАНЕЛЬ С СКОЛЬЗЯЩИМ ИНДИКАТОРОМ -->
      <nav class="h-14 flex-shrink-0 border-t border-slate-200/80 dark:border-slate-800/80 bg-white/95 dark:bg-slate-900/95 backdrop-blur px-3 sm:px-6 z-30" role="navigation" aria-label="Основная навигация">
        <div id="nav-tab-track" class="nav-tab-track max-w-md mx-auto h-full flex items-center justify-around gap-1" role="tablist">
          <div id="nav-tab-indicator" class="nav-tab-indicator"></div>
          
          <!-- Таб 1: Подключение -->
          <button
            type="button"
            id="nav-tab-connection"
            role="tab"
            aria-selected="true"
            aria-controls="panel-connection"
            tabindex="0"
            class="relative z-10 flex-1 py-1.5 px-2 rounded-xl flex flex-col items-center justify-center gap-0.5 text-blue-600 dark:text-blue-400 transition-colors focus:outline-none focus:ring-2 focus:ring-blue-500"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z"/></svg>
            <span class="text-[10px] font-semibold tracking-tight">Подключение</span>
          </button>

          <!-- Таб 2: Профиль -->
          <button
            type="button"
            id="nav-tab-profile"
            role="tab"
            aria-selected="false"
            aria-controls="panel-profile"
            tabindex="-1"
            class="relative z-10 flex-1 py-1.5 px-2 rounded-xl flex flex-col items-center justify-center gap-0.5 text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-white transition-colors focus:outline-none focus:ring-2 focus:ring-blue-500"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"/><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>
            <span class="text-[10px] font-semibold tracking-tight">Профиль</span>
          </button>

          <!-- Таб 3: Журнал -->
          <button
            type="button"
            id="nav-tab-logs"
            role="tab"
            aria-selected="false"
            aria-controls="panel-logs"
            tabindex="-1"
            class="relative z-10 flex-1 py-1.5 px-2 rounded-xl flex flex-col items-center justify-center gap-0.5 text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-white transition-colors relative focus:outline-none focus:ring-2 focus:ring-blue-500"
          >
            <div class="relative">
              <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"/></svg>
              <span id="nav-logs-badge" class="hidden absolute -top-1 -right-2 text-[9px] font-bold px-1 rounded-full bg-blue-600 text-white min-w-[14px] text-center leading-tight"></span>
            </div>
            <span class="text-[10px] font-semibold tracking-tight">Журнал</span>
          </button>

        </div>
      </nav>

      <!-- 5. ТОСТ (ВЫШЕ НАВИГАЦИИ) -->
      <div
        id="notification-toast"
        role="status"
        aria-live="polite"
        class="fixed bottom-16 sm:bottom-18 left-1/2 -translate-x-1/2 max-w-[90%] z-50 px-4 py-2 rounded-xl shadow-lg text-xs sm:text-sm font-medium transition-all duration-200 transform translate-y-3 opacity-0 pointer-events-none"
      ></div>

    </div>
  `;

  attachEventHandlers();
  setupRovingTabindex();
  populateManualFormValues();
  renderAllLogsFull();
  updateProfileViewStatus();
  updateMainTabSlidingIndicator();
  initSphere();
}

function initSphere() {
  if (sphereInstance) return;
  const canvas = document.getElementById('sphere-canvas') as HTMLCanvasElement | null;
  if (!canvas) return;

  const isDark =
    currentTheme === 'dark' ||
    (currentTheme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);

  sphereInstance = new MieruSphere(canvas, {
    isDark,
    initialState: 'cold_start',
  });

  // Cold start smoothly transitions to target status after ~1.4s assembly
  window.setTimeout(() => {
    isColdStartDone = true;
    updateSphereVisualState();
  }, 1400);
}

// ==========================================
// Roving Tabindex & Клавиатурная навигация
// ==========================================

function setupRovingTabindex() {
  const mainTabTrack = document.getElementById('nav-tab-track');
  if (mainTabTrack) {
    const mainTabs: AppTab[] = ['connection', 'profile', 'logs'];
    mainTabTrack.addEventListener('keydown', (e) => {
      const activeIdx = mainTabs.indexOf(activeTab);
      let targetIdx = -1;

      if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
        targetIdx = (activeIdx + 1) % mainTabs.length;
      } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
        targetIdx = (activeIdx - 1 + mainTabs.length) % mainTabs.length;
      } else if (e.key === 'Home') {
        targetIdx = 0;
      } else if (e.key === 'End') {
        targetIdx = mainTabs.length - 1;
      }

      if (targetIdx !== -1) {
        e.preventDefault();
        const nextTab = mainTabs[targetIdx];
        switchTab(nextTab);
        document.getElementById(`nav-tab-${nextTab}`)?.focus();
      }
    });
  }

  const subtabTrack = document.getElementById('subtab-track');
  if (subtabTrack) {
    const subModes: ProfileMode[] = ['manual', 'json'];
    subtabTrack.addEventListener('keydown', (e) => {
      const activeIdx = subModes.indexOf(activeProfileMode);
      let targetIdx = -1;

      if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
        targetIdx = (activeIdx + 1) % subModes.length;
      } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
        targetIdx = (activeIdx - 1 + subModes.length) % subModes.length;
      } else if (e.key === 'Home') {
        targetIdx = 0;
      } else if (e.key === 'End') {
        targetIdx = subModes.length - 1;
      }

      if (targetIdx !== -1) {
        e.preventDefault();
        const nextMode = subModes[targetIdx];
        switchProfileSubTab(nextMode);
        document.getElementById(nextMode === 'manual' ? 'subtab-manual' : 'subtab-json')?.focus();
      }
    });
  }
}

// ==========================================
// Слушатели событий интерфейса
// ==========================================

function attachEventHandlers() {
  // Тема
  document.getElementById('btn-toggle-theme')?.addEventListener('click', toggleTheme);

  // Навигационные вкладки
  document.getElementById('nav-tab-connection')?.addEventListener('click', () => switchTab('connection'));
  document.getElementById('nav-tab-profile')?.addEventListener('click', () => switchTab('profile'));
  document.getElementById('nav-tab-logs')?.addEventListener('click', () => switchTab('logs'));

  // Подвкладки профиля
  document.getElementById('subtab-manual')?.addEventListener('click', () => switchProfileSubTab('manual'));
  document.getElementById('subtab-json')?.addEventListener('click', () => switchProfileSubTab('json'));

  // Кнопка подключения/отключения
  document.getElementById('btn-connect-toggle')?.addEventListener('click', handleConnectToggle);

  // Ручная форма профиля
  const formManual = document.getElementById('form-profile-manual') as HTMLFormElement | null;
  formManual?.addEventListener('submit', (e) => handleSaveManualForm(e as SubmitEvent));
  formManual?.addEventListener('input', () => {
    userStartedEditingManual = true;
    syncManualDraftFromInputs();
    updateProfileViewStatus();
  });

  // Отслеживание ввода в JSON textarea
  const jsonTextarea = document.getElementById('textarea-json-import') as HTMLTextAreaElement | null;
  jsonTextarea?.addEventListener('input', () => {
    userStartedEditingJson = true;
    if (jsonTextarea) {
      jsonDraftText = jsonTextarea.value;
    }
    updateProfileViewStatus();
  });

  // Переключение видимости пароля
  document.getElementById('btn-toggle-password-visibility')?.addEventListener('click', () => {
    passwordVisible = !passwordVisible;
    const inputPwd = document.getElementById('input-password') as HTMLInputElement | null;
    const iconShow = document.getElementById('icon-pwd-show');
    const iconHide = document.getElementById('icon-pwd-hide');
    if (inputPwd) {
      inputPwd.type = passwordVisible ? 'text' : 'password';
    }
    if (passwordVisible) {
      iconShow?.classList.add('hidden');
      iconHide?.classList.remove('hidden');
    } else {
      iconShow?.classList.remove('hidden');
      iconHide?.classList.add('hidden');
    }
  });

  // Раскрытие дополнительных параметров с переключением inert
  const btnAdvanced = document.getElementById('btn-toggle-advanced');
  btnAdvanced?.addEventListener('click', () => {
    isAdvancedOpen = !isAdvancedOpen;
    const box = document.getElementById('box-advanced');
    const arrow = document.getElementById('icon-arrow-advanced');
    if (isAdvancedOpen) {
      box?.classList.remove('is-closed');
      box?.classList.add('is-open');
      box?.removeAttribute('inert');
      arrow?.classList.add('rotate-90');
      btnAdvanced.setAttribute('aria-expanded', 'true');
    } else {
      box?.classList.remove('is-open');
      box?.classList.add('is-closed');
      box?.setAttribute('inert', '');
      arrow?.classList.remove('rotate-90');
      btnAdvanced.setAttribute('aria-expanded', 'false');
    }
  });

  // Пример из документации
  document.getElementById('btn-load-example')?.addEventListener('click', () => {
    userStartedEditingManual = true;
    userStartedEditingJson = true;
    loadExampleProfile();
  });

  // JSON импорт
  document.getElementById('btn-submit-json')?.addEventListener('click', () => {
    const textarea = document.getElementById('textarea-json-import') as HTMLTextAreaElement | null;
    if (textarea && textarea.value.trim()) {
      handleImportJson(textarea.value.trim());
    } else {
      showNotification('warning', 'Вставьте JSON-строку профиля в поле ввода');
    }
  });

  document.getElementById('btn-paste-example-json')?.addEventListener('click', () => {
    userStartedEditingJson = true;
    const textarea = document.getElementById('textarea-json-import') as HTMLTextAreaElement | null;
    const example = {
      server: '203.0.113.10',
      server_port: 8964,
      transport: 'TCP',
      username: 'baozi',
      password: 'manlianpenfen',
      traffic_pattern: 'GgQIARAK',
    };
    if (textarea) {
      textarea.value = JSON.stringify(example, null, 2);
      jsonDraftText = textarea.value;
      updateProfileViewStatus();
    }
  });

  const fileInput = document.getElementById('input-file-json') as HTMLInputElement | null;
  document.getElementById('btn-pick-file-json')?.addEventListener('click', () => fileInput?.click());
  fileInput?.addEventListener('change', (e) => {
    const target = e.target as HTMLInputElement;
    const file = target.files?.[0];
    if (file) {
      const reader = new FileReader();
      reader.onload = (event) => {
        const text = String(event.target?.result || '');
        const textarea = document.getElementById('textarea-json-import') as HTMLTextAreaElement | null;
        if (textarea) textarea.value = text;
        jsonDraftText = text;
        userStartedEditingJson = true;
        handleImportJson(text);
      };
      reader.readAsText(file);
    }
  });

  // Очистка логов
  document.getElementById('btn-clear-logs')?.addEventListener('click', () => {
    logs = [];
    seenCoreLogLines.clear();
    const container = document.getElementById('logs-container');
    if (container) {
      container.innerHTML =
        '<div class="logs-empty-placeholder text-slate-400 dark:text-slate-600 text-center py-6 select-none">Журнал событий пуст</div>';
    }
    updateLogsCountBadge();
  });

  window.addEventListener('resize', () => {
    updateMainTabSlidingIndicator();
    updateSubTabSlidingIndicator();
    sphereInstance?.resize();
  });

  // Горячие клавиши (View/Profile shortcuts и закрытие уведомлений по Escape)
  window.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      const toast = document.getElementById('notification-toast');
      if (toast && !toast.classList.contains('opacity-0')) {
        toast.className =
          'fixed bottom-16 sm:bottom-18 left-1/2 -translate-x-1/2 max-w-[90%] z-50 px-4 py-2 rounded-xl shadow-lg text-xs sm:text-sm font-medium transition-all duration-200 transform translate-y-3 opacity-0 pointer-events-none';
      }
      return;
    }

    // In native Tauri environment, accelerators are managed by the native application menu events
    if (isNative) return;

    // In browser: only standard primary modifier, ignore Shift/Alt variants
    if (e.shiftKey || e.altKey) return;
    const isMac = typeof navigator !== 'undefined' && /mac/i.test(navigator.userAgent || navigator.platform || '');
    const isPrimaryMod = isMac ? (e.metaKey && !e.ctrlKey) : (e.ctrlKey && !e.metaKey);
    if (!isPrimaryMod) return;

    if (e.key === '1') {
      e.preventDefault();
      switchTab('connection');
    } else if (e.key === '2') {
      e.preventDefault();
      switchTab('profile');
    } else if (e.key === '3') {
      e.preventDefault();
      switchTab('logs');
    } else if (e.key.toLowerCase() === 's') {
      e.preventDefault();
      switchTab('profile');
      if (activeProfileMode === 'manual') {
        const form = document.getElementById('form-profile-manual') as HTMLFormElement | null;
        if (form) {
          form.requestSubmit ? form.requestSubmit() : form.dispatchEvent(new Event('submit', { cancelable: true }));
        }
      } else {
        document.getElementById('btn-submit-json')?.click();
      }
    } else if (e.key.toLowerCase() === 'o') {
      e.preventDefault();
      switchTab('profile');
      switchProfileSubTab('json');
      const fileInput = document.getElementById('input-file-json') as HTMLInputElement | null;
      fileInput?.click();
    }
  });
}

// ==========================================
// Визуальные элементы статуса и кнопок
// ==========================================

function renderStateBadge(state: VpnState): string {
  const map: Record<VpnState, { text: string; bg: string }> = {
    disconnected: { text: 'Отключено', bg: 'bg-slate-200/80 dark:bg-slate-800 text-slate-700 dark:text-slate-300' },
    connecting: {
      text: 'Подключение...',
      bg: 'bg-amber-100 dark:bg-amber-950/80 text-amber-700 dark:text-amber-300 animate-soft-pulse',
    },
    connected: {
      text: 'Подключено',
      bg: 'bg-emerald-100 dark:bg-emerald-950/80 text-emerald-700 dark:text-emerald-300 font-semibold',
    },
    disconnecting: {
      text: 'Отключение...',
      bg: 'bg-amber-100 dark:bg-amber-950/80 text-amber-700 dark:text-amber-300 animate-soft-pulse',
    },
    error: { text: 'Ошибка', bg: 'bg-rose-100 dark:bg-rose-950/80 text-rose-700 dark:text-rose-300 font-semibold' },
  };
  const item = map[state] || map.disconnected;
  return `<span class="text-[11px] px-2 py-0.5 rounded-full ${item.bg}">${item.text}</span>`;
}


function renderStatusTitle(state: VpnState): string {
  switch (state) {
    case 'connected':
      return 'Подключено';
    case 'connecting':
      return 'Подключение...';
    case 'disconnecting':
      return 'Отключение...';
    case 'error':
      return 'Ошибка соединения';
    case 'disconnected':
    default:
      if (!savedProfile) {
        return 'Профиль не настроен';
      }
      if (serverHealth?.state === 'error') {
        return 'Сервер недоступен';
      }
      if (serverHealth?.state === 'ready') {
        return 'Готов к подключению';
      }
      return 'Отключено';
  }
}

function renderStatusSubtitle(state: VpnState, message: string): string {
  switch (state) {
    case 'connected':
      if (serverHealth?.state === 'ready') {
        return '';
      }
      if (serverHealth?.state === 'error') {
        return `Туннель активен (проверка сервера: ${serverHealth.message || 'ошибка ответа'})`;
      }
      return 'Туннель активен, проверка сервера...';

    case 'connecting':
      return 'Установка соединения и запуск сетевого туннеля...';

    case 'disconnecting':
      return 'Завершение сессии и восстановление сетевого подключения...';

    case 'error':
      return message ? 'Возникла неполадка при работе туннеля' : 'Произошла непредвиденная ошибка';

    case 'disconnected':
    default:
      if (!savedProfile) {
        return 'Настройте и сохраните профиль сервера для подключения';
      }
      if (serverHealth?.state === 'ready') {
        return '';
      }
      if (serverHealth?.state === 'error') {
        return `Сервер недоступен: ${serverHealth.message || 'нет связи'}`;
      }
      if (isCheckingHealth) {
        return 'Проверка доступности сервера...';
      }
      return 'Трафик направляется через обычное интернет-соединение';
  }
}

function getConnectButtonClasses(state: VpnState): string {
  return `connect-button connect-button--${state}`;
}

function getStatusMessageTone(): string {
  if (vpnStatus.state === 'connecting' || vpnStatus.state === 'disconnecting' || isCheckingHealth) return 'waiting';
  if (vpnStatus.state === 'error' || serverHealth?.state === 'error') return 'error';
  if (vpnStatus.state === 'connected' && serverHealth?.state === 'ready') return 'success';
  if (serverHealth?.state === 'ready') return 'ready';
  return 'neutral';
}

function renderConnectButtonContent(state: VpnState): string {
  switch (state) {
    case 'connected':
      return `
        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"/></svg>
        <span>Отключить</span>
      `;
    case 'connecting':
      return `
        <svg class="w-5 h-5 animate-spin" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"/></svg>
        <span>Подключение...</span>
      `;
    case 'disconnecting':
      return `
        <svg class="w-5 h-5 animate-spin" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"/></svg>
        <span>Отключение...</span>
      `;
    case 'error':
    case 'disconnected':
    default:
      return `
        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z"/></svg>
        <span>Подключить</span>
      `;
  }
}

function renderThemeIcon(theme: ThemeMode): string {
  switch (theme) {
    case 'light':
      return `<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z"/></svg>`;
    case 'dark':
      return `<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z"/></svg>`;
    case 'system':
    default:
      return `<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z"/></svg>`;
  }
}

// ==========================================
// Первоначальный запуск приложения
// ==========================================

document.addEventListener('DOMContentLoaded', async () => {
  document.title = APP_NAME;
  applyTheme(currentTheme);
  setupSystemThemeListener(() => {
    updateThemeButtonUI();
    const isDark =
      currentTheme === 'dark' ||
      (currentTheme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
    sphereInstance?.setDark(isDark);
  });

  renderInitialLayout();

  addLog(
    'info',
    isNative
      ? 'Приложение запущено в нативном окружении Tauri'
      : 'Приложение запущено в браузере (нативный бэкенд недоступен)'
  );

  // 1. Подписка на события Tauri vpn-status и нативного меню
  if (isNative) {
    try {
      await onVpnStatusChange((status) => {
        updateStatus(status);
      });
      addLog('info', 'Подписка на события статуса vpn-status активна');
    } catch (err) {
      addLog(
        'error',
        `Ошибка подписки на vpn-status: ${err instanceof Error ? err.message : String(err)}`
      );
    }

    try {
      const unlistenNav = await listen<string>('menu-nav', (event) => {
        if (event.payload === 'connection' || event.payload === 'profile' || event.payload === 'logs') {
          switchTab(event.payload);
        }
      });

      const unlistenSave = await listen('menu-profile-save', () => {
        switchTab('profile');
        if (activeProfileMode === 'manual') {
          const form = document.getElementById('form-profile-manual') as HTMLFormElement | null;
          if (form) {
            form.requestSubmit ? form.requestSubmit() : form.dispatchEvent(new Event('submit', { cancelable: true }));
          }
        } else {
          document.getElementById('btn-submit-json')?.click();
        }
      });

      const unlistenImport = await listen('menu-profile-import-json', () => {
        switchTab('profile');
        switchProfileSubTab('json');
        const fileInput = document.getElementById('input-file-json') as HTMLInputElement | null;
        fileInput?.click();
      });

      window.addEventListener(
        'beforeunload',
        () => {
          unlistenNav();
          unlistenSave();
          unlistenImport();
        },
        { once: true }
      );
      addLog('info', 'Подписка на события нативного меню активна');
    } catch (err) {
      addLog(
        'warn',
        `Не удалось подключить слушатели нативного меню: ${err instanceof Error ? err.message : String(err)}`
      );
    }
  }

  // 2. Первоначальная асинхронная загрузка сохранённого профиля и статуса
  try {
    const [saved, initialStatus] = await Promise.all([
      getProfile().catch((err) => {
        addLog('warn', `Не удалось загрузить профиль: ${err.message}`);
        return null;
      }),
      getVpnStatus().catch((err) => {
        addLog('warn', `Не удалось запросить статус VPN: ${err.message}`);
        return {
          state: 'disconnected' as VpnState,
          message: '',
          since: new Date().toISOString(),
        };
      }),
    ]);

    if (saved) {
      savedProfile = { ...saved };
      sessionPassword = saved.password || '';

      if (!userStartedEditingManual) {
        manualDraft = {
          server: saved.server,
          server_port: saved.server_port,
          transport: saved.transport,
          username: saved.username,
          password: sessionPassword,
          traffic_pattern: saved.traffic_pattern || '',
        };
        populateManualFormValues();
      }

      if (!userStartedEditingJson) {
        jsonDraftText = JSON.stringify(saved, null, 2);
        const jsonTextarea = document.getElementById('textarea-json-import') as HTMLTextAreaElement | null;
        if (jsonTextarea) {
          jsonTextarea.value = jsonDraftText;
        }
      }

      addLog('info', `Загружен активный профиль: ${saved.server}:${saved.server_port} (${saved.transport})`);
    }

    if (initialStatus) {
      updateStatus(initialStatus);
    }
  } catch (err) {
    addLog(
      'error',
      `Ошибка инициализации: ${err instanceof Error ? err.message : String(err)}`
    );
  }

  // 3. Запуск опроса журнала
  startLogPolling();

  updateProfileViewStatus();
  updateConnectionScreenView();

  // 4. Первоначальная проверка доступности сервера (если есть сохранённый профиль)
  if (savedProfile) {
    runServerHealthCheck(false);
  }
});

// ==========================================
// Dev QA API для тестирования состояний сферы и Health Check
// ==========================================
if (import.meta.env.DEV && typeof window !== 'undefined') {
  (window as unknown as { __MIERU_QA__: unknown }).__MIERU_QA__ = {
    get sphere() {
      return sphereInstance;
    },
    setSphereState: (s: SphereVisualState) => {
      isColdStartDone = true;
      sphereInstance?.setState(s);
    },
    runHealthCheck: (force = true) => runServerHealthCheck(force),
    getStatus: () => vpnStatus,
    getHealth: () => serverHealth,
    getSafeProfile: () => {
      if (!savedProfile) return null;
      return {
        server: savedProfile.server,
        server_port: savedProfile.server_port,
        transport: savedProfile.transport,
        username: savedProfile.username,
      };
    },
  };
}
