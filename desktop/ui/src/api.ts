import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { MieruProfile, ServerHealth, VpnStatus } from './types';

/**
 * Проверка, запущено ли приложение внутри окружения Tauri
 */
export function isTauriEnvironment(): boolean {
  if (typeof window === 'undefined') return false;
  return '__TAURI_INTERNALS__' in window || '__TAURI__' in window;
}

/**
 * Получение текущего сохранённого профиля
 */
export async function getProfile(): Promise<MieruProfile | null> {
  if (!isTauriEnvironment()) {
    return null;
  }
  try {
    const profile = await invoke<MieruProfile | null>('profile_get');
    return profile;
  } catch (err) {
    throw new Error(`Ошибка получения профиля: ${err instanceof Error ? err.message : String(err)}`);
  }
}

/**
 * Сохранение профиля
 */
export async function setProfile(profile: MieruProfile): Promise<void> {
  if (!isTauriEnvironment()) {
    throw new Error('Нативный бэкенд недоступен в обычном браузере. Запустите приложение через Tauri.');
  }
  try {
    await invoke('profile_set', { profile });
  } catch (err) {
    throw new Error(`Ошибка сохранения профиля: ${err instanceof Error ? err.message : String(err)}`);
  }
}

/**
 * Подключение VPN
 */
export async function connectVpn(): Promise<void> {
  if (!isTauriEnvironment()) {
    throw new Error('Подключение невозможно: нативный бэкенд недоступен в браузере.');
  }
  try {
    await invoke('vpn_connect');
  } catch (err) {
    throw new Error(`Ошибка подключения: ${err instanceof Error ? err.message : String(err)}`);
  }
}

/**
 * Отключение VPN
 */
export async function disconnectVpn(): Promise<void> {
  if (!isTauriEnvironment()) {
    throw new Error('Отключение невозможно: нативный бэкенд недоступен в браузере.');
  }
  try {
    await invoke('vpn_disconnect');
  } catch (err) {
    throw new Error(`Ошибка отключения: ${err instanceof Error ? err.message : String(err)}`);
  }
}

/**
 * Запрос текущего статуса VPN
 */
export async function getVpnStatus(): Promise<VpnStatus> {
  if (!isTauriEnvironment()) {
    return {
      state: 'disconnected',
      message: 'Нативный бэкенд недоступен (режим браузера)',
      since: new Date().toISOString(),
    };
  }
  try {
    const status = await invoke<VpnStatus>('vpn_status_get');
    return status;
  } catch (err) {
    return {
      state: 'error',
      message: `Ошибка получения статуса: ${err instanceof Error ? err.message : String(err)}`,
      since: new Date().toISOString(),
    };
  }
}

/**
 * Подписка на событие изменения статуса vpn-status
 */
export async function onVpnStatusChange(callback: (status: VpnStatus) => void): Promise<UnlistenFn> {
  if (!isTauriEnvironment()) {
    // В браузере возвращаем no-op функцию отписки
    return () => {};
  }
  return await listen<VpnStatus>('vpn-status', (event) => {
    callback(event.payload);
  });
}

/**
 * Получение последних строк журнала ядра
 */
export async function getVpnLogs(): Promise<{ lines: string[] }> {
  if (!isTauriEnvironment()) {
    return { lines: [] };
  }
  try {
    return await invoke<{ lines: string[] }>('vpn_log_get');
  } catch {
    return { lines: [] };
  }
}

/**
 * Проверка доступности сервера (health check через loopback mixed proxy)
 */
export async function checkServerHealth(): Promise<ServerHealth> {
  if (!isTauriEnvironment()) {
    // В браузере возвращаем unconfigured без реальных запросов
    return {
      state: 'unconfigured',
      latency_ms: null,
      message: 'Проверка доступна только в десктопном приложении',
      checked_at: new Date().toISOString(),
    };
  }
  try {
    const health = await invoke<ServerHealth>('server_health_check');
    return health;
  } catch (err) {
    return {
      state: 'error',
      latency_ms: null,
      message: err instanceof Error ? err.message : String(err),
      checked_at: new Date().toISOString(),
    };
  }
}
