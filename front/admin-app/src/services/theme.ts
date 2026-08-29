import { resolveAppPath } from '@/services/baseUrl'

/** Theme mode options. */
export type ThemeMode = 'system' | 'light' | 'dark'

/** Theme preset definition. */
export interface ThemePreset {
  /** Machine-readable theme identifier used for persistence. */
  key: string
  /** Human-readable theme name shown in the UI. */
  label: string
  /** Vuetify theme variant: 'light' or 'dark'. */
  variant: 'light' | 'dark'
}

/** Available theme presets. */
const THEME_PRESETS: ThemePreset[] = [
  { key: 'system', label: 'System', variant: 'light' },
  { key: 'light', label: 'Light', variant: 'light' },
  { key: 'dark', label: 'Dark', variant: 'dark' },
]

/** Default theme key used when no preference is stored. */
export const DEFAULT_THEME_KEY = 'system'

/** Storage key for the theme preference. */
const THEME_STORAGE_KEY = 'admin.theme'

/**
 * Returns all available theme presets.
 *
 * @returns Array of theme preset definitions.
 */
export function getThemePresets(): ThemePreset[] {
  return THEME_PRESETS
}

/**
 * Finds a theme preset by its key.
 *
 * @param key - Theme preset key to find.
 * @returns Matching theme preset, or the system default.
 */
export function findThemePreset(key: string): ThemePreset {
  return THEME_PRESETS.find((preset) => preset.key === key) ?? THEME_PRESETS[0]!
}

/**
 * Resolves the effective Vuetify theme variant for a preset key.
 * System mode uses the OS preference via matchMedia.
 *
 * @param key - Theme preset key.
 * @returns The Vuetify theme variant ('light' or 'dark').
 */
export function resolveEffectiveVariant(key: string): 'light' | 'dark' {
  if (key === 'system') {
    return typeof window !== 'undefined' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches
      ? 'dark'
      : 'light'
  }
  return findThemePreset(key).variant
}

/**
 * Loads the persisted theme preference from local storage.
 *
 * @returns The stored theme key, or the default.
 */
export function loadStoredTheme(): ThemeMode {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY)
    if (stored && THEME_PRESETS.some((preset) => preset.key === stored)) {
      return stored as ThemeMode
    }
  } catch {
    // localStorage unavailable; fall through to default.
  }
  return DEFAULT_THEME_KEY
}

/**
 * Persists the theme preference to local storage.
 *
 * @param key - Theme preset key to store.
 */
export function persistTheme(key: string): void {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, key)
  } catch {
    // localStorage unavailable; ignore.
  }
}

// Re-export for convenience.
export { resolveAppPath }
