export const STELLAR_THEME_KEY = 'stellar:theme';

const THEMES = ['default', 'dark', 'cosmic', 'corporate'];

export function resolveInitialTheme(): string {
  try {
    const saved = localStorage.getItem(STELLAR_THEME_KEY);
    if (saved && THEMES.includes(saved)) {
      return saved;
    }
  } catch {
  }
  return 'dark';
}

export function persistTheme(themeName: string): void {
  try {
    localStorage.setItem(STELLAR_THEME_KEY, themeName);
  } catch {
  }
}
