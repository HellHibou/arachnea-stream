/**
 * Shape of a theme preset exposed by the frontend theme system.
 *
 * Each preset maps to an HSL primary color that drives the
 * `--color-primary` CSS custom property and its derived accents.
 */
import { resolveAppPath } from '@/services/baseUrl'

export interface ThemePreset {
  /** Machine-readable theme identifier used for persistence. */
  key: string
  /** Human-readable theme name shown in settings and tooltips. */
  label: string
  /** Hue value passed to `hsl()` for the primary color. */
  h: number
  /** Saturation value passed to `hsl()` for the primary color. */
  s: string
  /** Lightness value passed to `hsl()` for the primary color. */
  l: string
}

/**
 * Runtime theme presets loaded from `public/themes.json`.
 *
 * Starts empty and is populated by {@link loadThemes} during app startup.
 * The array reference stays stable so consumers holding a reference
 * (e.g. the `useTheme` composable) see updates without re-binding.
 */
export const themePresets: ThemePreset[] = []

/** Default theme key used when no preference is stored. */
export const defaultThemeKey = 'arachnea-blue'

/** Fallback theme preset used when `themes.json` cannot be loaded. */
const FALLBACK_THEME: ThemePreset = {
  key: 'arachnea-blue',
  label: 'Arachnéa Blue',
  h: 210,
  s: '100%',
  l: '60%',
}

/**
 * Loads theme presets from the public `themes.json` asset.
 *
 * Call this once during app startup, before applying the stored theme
 * preference. Subsequent calls are safe but will re-fetch the asset.
 *
 * When the network request fails or the payload is invalid, the function
 * falls back to a single default theme so the app can still render.
 *
 * @returns Promise resolving to the parsed theme presets.
 */
export async function loadThemes(): Promise<ThemePreset[]> {
  try {
    const response = await fetch(resolveAppPath('themes.json'))
    if (!response.ok) {
      throw new Error(`Unable to load themes (${response.status}).`)
    }

    const payload: unknown = await response.json()
    if (!Array.isArray(payload)) {
      throw new Error('Invalid themes format.')
    }

    themePresets.length = 0
    for (const item of payload) {
      if (isThemePreset(item)) {
        themePresets.push(item)
      }
    }
  } catch {
    themePresets.length = 0
    themePresets.push(FALLBACK_THEME)
  }

  return themePresets
}

/**
 * Returns the theme preset matching the given key, or the first preset.
 *
 * @param key Theme preset key to find.
 * @returns Matching theme preset, or the first available preset as fallback.
 */
export function findThemePreset(key: string): ThemePreset {
  return themePresets.find((preset) => preset.key === key) ?? themePresets[0]!
}

/**
 * Applies a theme preset by setting CSS custom properties on the document root.
 *
 * @param preset Theme preset to apply.
 */
export function applyTheme(preset: ThemePreset): void {
  const root = document.documentElement
  root.style.setProperty('--h-primary', String(preset.h))
  root.style.setProperty('--s-primary', preset.s)
  root.style.setProperty('--l-primary', preset.l)
}

/**
 * Validates whether a value matches the {@link ThemePreset} shape.
 *
 * @param value Value to validate.
 * @returns True when the value is a valid theme preset.
 */
function isThemePreset(value: unknown): value is ThemePreset {
  if (typeof value !== 'object' || value === null) {
    return false
  }

  const record = value as Record<string, unknown>
  return (
    typeof record.key === 'string' &&
    typeof record.label === 'string' &&
    typeof record.h === 'number' &&
    typeof record.s === 'string' &&
    typeof record.l === 'string'
  )
}
