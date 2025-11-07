const themeTokens = {
  "--nr-text-color": "rgb(226, 230, 246)",
  "--nr-text-secondary": "rgba(226, 230, 246, 0.65)",
  "--text-secondary": "rgba(226, 230, 246, 0.65)",
  "--nr-background-primary": "rgb(7, 10, 23)",
  "--nr-background-secondary": "rgb(18, 24, 44)",
  "--nr-background-tertiary": "rgb(26, 33, 58)",
  "--nr-border-color": "rgba(226, 230, 246, 0.15)",
  "--border-color": "rgba(226, 230, 246, 0.15)",
  "--nr-primary-color": "rgb(138, 163, 219)",
  "--nr-primary-color-strong": "rgba(138, 163, 219, 0.85)",
  "--nr-primary-color-soft": "rgba(138, 163, 219, 0.18)",
  "--nr-accent-color": "rgb(201, 81, 114)",
  "--nr-accent-color-soft": "rgba(201, 81, 114, 0.18)",
  "--nr-success-color": "rgb(144, 238, 144)",
  "--error-color": "rgb(225, 43, 43)",
  "--nr-focus-ring": "rgba(138, 163, 219, 0.35)",
  "--nr-input-background": "rgba(26, 33, 58, 0.92)",
  "--nr-input-border": "rgba(226, 230, 246, 0.22)",
  "--nr-input-placeholder": "rgba(226, 230, 246, 0.55)",
  "--nr-table-row-hover": "rgba(138, 163, 219, 0.12)",
} as const;

type ThemeTokens = typeof themeTokens;

export { themeTokens };

export function applyThemeTokens(target: HTMLElement = document.documentElement): void {
  const style = target.style;
  for (const [variable, value] of Object.entries(themeTokens)) {
    style.setProperty(variable, value);
  }
}

export type ThemeTokenName = keyof ThemeTokens;
