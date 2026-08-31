import { computed, watch } from 'vue'

import { themePresets, findThemePreset, applyTheme } from '@/services/theme'
import { useStorage } from '@/services/storage'

/**
 * Provides the active theme preset and a function to switch themes.
 *
 * The selected theme is persisted in localStorage and applied by setting
 * CSS custom properties on the document element.
 */
export function useTheme() {
  const storage = useStorage()
  const themeKey = storage.getParameters().theme

  /** The currently active theme preset. */
  const currentPreset = computed(() => findThemePreset(themeKey.value))

  /** Available theme presets. */
  const presets = themePresets

  /** Whether the theme selector should be displayed. */
  const isVisible = computed(() => presets.length > 1)

  /**
   * Switches the active theme and persists the selection.
   *
   * @param key Theme preset key to activate.
   */
  function setTheme(key: string): void {
    themeKey.value = key
    applyTheme(findThemePreset(key))
  }

  watch(themeKey, (next) => {
    applyTheme(findThemePreset(next))
  })

  return {
    currentPreset,
    presets,
    setTheme,
    isVisible,
  }
}