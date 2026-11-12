import type { ThemeMode } from './types';

const THEME_STORAGE_KEY = 'mieru_theme';

export function getInitialTheme(): ThemeMode {
  const stored = localStorage.getItem(THEME_STORAGE_KEY) as ThemeMode | null;
  if (stored === 'light' || stored === 'dark' || stored === 'system') {
    return stored;
  }
  return 'system';
}

export function applyTheme(mode: ThemeMode): void {
  localStorage.setItem(THEME_STORAGE_KEY, mode);
  const root = document.documentElement;

  if (mode === 'dark') {
    root.classList.add('dark');
  } else if (mode === 'light') {
    root.classList.remove('dark');
  } else {
    // system
    const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    if (prefersDark) {
      root.classList.add('dark');
    } else {
      root.classList.remove('dark');
    }
  }
}

export function setupSystemThemeListener(onThemeChange: () => void): void {
  const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
  mediaQuery.addEventListener('change', () => {
    const currentMode = getInitialTheme();
    if (currentMode === 'system') {
      applyTheme('system');
      onThemeChange();
    }
  });
}
