import type { MessageKey } from "@front-shared/i18n";

export type ThemeId = "classic" | "mica" | "pine" | "sea-salt" | "jade-night";

export interface ThemeOption {
  id: ThemeId;
  nameKey: MessageKey;
  descriptionKey: MessageKey;
}

const STORAGE_KEY = "codex-switch-theme";
const DEFAULT_THEME: ThemeId = "classic";

export const themeOptions: readonly ThemeOption[] = [
  {
    id: "classic",
    nameKey: "themeClassicName",
    descriptionKey: "themeClassicDescription",
  },
  {
    id: "mica",
    nameKey: "themeMicaName",
    descriptionKey: "themeMicaDescription",
  },
  {
    id: "pine",
    nameKey: "themePineName",
    descriptionKey: "themePineDescription",
  },
  {
    id: "sea-salt",
    nameKey: "themeSeaSaltName",
    descriptionKey: "themeSeaSaltDescription",
  },
  {
    id: "jade-night",
    nameKey: "themeJadeNightName",
    descriptionKey: "themeJadeNightDescription",
  },
] as const;

export function isThemeId(value: string | undefined): value is ThemeId {
  return themeOptions.some((theme) => theme.id === value);
}

export function getThemeOption(themeId: ThemeId): ThemeOption {
  return themeOptions.find((theme) => theme.id === themeId) ?? themeOptions[0];
}

export function resolveInitialTheme(): ThemeId {
  const stored = globalThis.localStorage?.getItem(STORAGE_KEY) ?? undefined;
  return isThemeId(stored) ? stored : DEFAULT_THEME;
}

export function persistTheme(theme: ThemeId): void {
  globalThis.localStorage?.setItem(STORAGE_KEY, theme);
}

export function applyTheme(theme: ThemeId): void {
  document.documentElement.dataset.theme = theme;
}
