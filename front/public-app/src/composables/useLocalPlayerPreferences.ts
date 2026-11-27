import { readonly, ref } from 'vue'
import { getStoreItem, setStoreItem } from '@/services/storage/persistence'
import { sanitizeBoolean } from '@/services/storage/sanitizers'

/** Device-only preference, deliberately separate from profile preference documents. */
const CINEMA_MODE_DEFAULT_KEY = 'videoPlayer.preferences.cinemaModeByDefault'
const cinemaModeByDefault = ref(sanitizeBoolean(getStoreItem<unknown>(CINEMA_MODE_DEFAULT_KEY), false))

/**
 * Provides player layout defaults stored only in this browser or desktop webview.
 * @returns Readonly cinema default and an explicit local persistence action.
 */
export function useLocalPlayerPreferences() {
  /** Saves the default without updating the current entry's manual layout choice.
   * @param enabled Whether newly opened entries should start in cinema mode.
   */
  function setCinemaModeByDefault(enabled: boolean): void {
    cinemaModeByDefault.value = enabled
    setStoreItem(CINEMA_MODE_DEFAULT_KEY, enabled)
  }

  return { cinemaModeByDefault: readonly(cinemaModeByDefault), setCinemaModeByDefault }
}
