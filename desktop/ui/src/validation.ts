import type { MieruProfile, TransportType } from './types';

/**
 * Валидация base64 строки (для traffic_pattern)
 */
export function isValidBase64(str: string): boolean {
  if (!str || typeof str !== 'string') return false;
  const trimmed = str.trim();
  if (trimmed.length === 0) return false;
  // Проверяем допустимые символы base64
  const base64Regex = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;
  return base64Regex.test(trimmed);
}

/**
 * Валидация полей профиля
 */
export function validateProfileFields(
  profile: Partial<MieruProfile>,
  requirePassword = true
): { valid: boolean; errors: Record<string, string> } {
  const errors: Record<string, string> = {};

  // server
  if (!profile.server || typeof profile.server !== 'string' || profile.server.trim().length === 0) {
    errors.server = 'Укажите адрес сервера (IP-адрес или доменное имя)';
  }

  // server_port
  const port = Number(profile.server_port);
  if (!profile.server_port || isNaN(port) || !Number.isInteger(port) || port < 1 || port > 65535) {
    errors.server_port = 'Порт должен быть целым числом в диапазоне 1–65535';
  }

  // transport
  if (!profile.transport || !['TCP', 'UDP'].includes(profile.transport)) {
    errors.transport = 'Протокол транспорта должен быть TCP или UDP';
  }

  // username
  if (!profile.username || typeof profile.username !== 'string' || profile.username.trim().length === 0) {
    errors.username = 'Укажите имя пользователя';
  }

  // password
  if (requirePassword) {
    if (!profile.password || typeof profile.password !== 'string' || profile.password.length === 0) {
      errors.password = 'Пароль обязателен для создания или обновления профиля';
    }
  }

  // traffic_pattern (необязательное поле)
  if (profile.traffic_pattern && profile.traffic_pattern.trim().length > 0) {
    if (!isValidBase64(profile.traffic_pattern.trim())) {
      errors.traffic_pattern = 'Шаблон трафика должен быть корректной строкой Base64';
    }
  }

  return {
    valid: Object.keys(errors).length === 0,
    errors,
  };
}

/**
 * Валидация сырого JSON профиля согласно configs/profile.schema.json
 */
export function validateProfileJson(rawJson: string): {
  valid: boolean;
  profile?: MieruProfile;
  error?: string;
  errors?: Record<string, string>;
} {
  let parsed: unknown;
  try {
    parsed = JSON.parse(rawJson);
  } catch (e) {
    return {
      valid: false,
      error: 'Некорректный синтаксис JSON: ' + (e instanceof Error ? e.message : String(e)),
    };
  }

  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    return {
      valid: false,
      error: 'JSON профиля должен быть объектом { ... }',
    };
  }

  const obj = parsed as Record<string, unknown>;

  // Проверка additionalProperties: false
  const allowedKeys = new Set(['server', 'server_port', 'transport', 'username', 'password', 'traffic_pattern']);
  const unknownKeys = Object.keys(obj).filter((k) => !allowedKeys.has(k));
  if (unknownKeys.length > 0) {
    return {
      valid: false,
      error: `Недопустимые дополнительные поля в профиле: ${unknownKeys.join(', ')}`,
    };
  }

  // Строгая проверка типов полей JSON профиля (без скрытого приведения типов)
  if ('server' in obj && typeof obj.server !== 'string') {
    return { valid: false, error: 'Поле server должно быть строкой' };
  }
  if ('server_port' in obj && (typeof obj.server_port !== 'number' || !Number.isInteger(obj.server_port))) {
    return { valid: false, error: 'Поле server_port должно быть целым числом, а не строкой' };
  }
  if ('transport' in obj && typeof obj.transport !== 'string') {
    return { valid: false, error: 'Поле transport должно быть строкой "TCP" или "UDP"' };
  }
  if ('username' in obj && typeof obj.username !== 'string') {
    return { valid: false, error: 'Поле username должно быть строкой' };
  }
  if ('password' in obj && typeof obj.password !== 'string') {
    return { valid: false, error: 'Поле password должно быть строкой' };
  }
  if ('traffic_pattern' in obj && typeof obj.traffic_pattern !== 'string') {
    return { valid: false, error: 'Поле traffic_pattern должно быть строкой' };
  }

  // Построение кандидата профиля
  const candidate: Partial<MieruProfile> = {
    server: typeof obj.server === 'string' ? obj.server.trim() : undefined,
    server_port: typeof obj.server_port === 'number' ? obj.server_port : undefined,
    transport: typeof obj.transport === 'string' ? (obj.transport.toUpperCase() as TransportType) : undefined,
    username: typeof obj.username === 'string' ? obj.username.trim() : undefined,
    password: typeof obj.password === 'string' ? obj.password : undefined,
    traffic_pattern: typeof obj.traffic_pattern === 'string' ? obj.traffic_pattern.trim() : undefined,
  };

  const { valid, errors } = validateProfileFields(candidate, true);

  if (!valid) {
    const firstError = Object.values(errors)[0];
    return {
      valid: false,
      error: firstError,
      errors,
    };
  }

  return {
    valid: true,
    profile: candidate as MieruProfile,
  };
}
