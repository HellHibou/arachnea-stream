import { onScopeDispose, shallowRef, toValue, watch, type MaybeRefOrGetter } from 'vue'
import { loadCustomHome } from '@/services/customHomes'
import type { CustomHomeDefinition } from '@/types/customHome'
import { t } from '@/i18n'

/**
 * Reads persisted definitions and refreshes on focus/visibility across browser/native views.
 * @param homeId Stable route id.
 * @returns Current definition, read failure and refresh action guarded against stale reads.
 */
export function useCustomHomeDefinition(homeId: MaybeRefOrGetter<string>) {
  const definition = shallowRef<CustomHomeDefinition | null>(null)
  const errorMessage = shallowRef<string | null>(null)
  const isLoading = shallowRef(false)
  let revision = 0

  /** Reads the active id without ever falling back to another home or provider. */
  async function refresh(): Promise<void> {
    const request = ++revision
    const id = toValue(homeId)
    if (!id) {
      definition.value = null
      errorMessage.value = null
      isLoading.value = false
      return
    }
    isLoading.value = true
    errorMessage.value = null
    try {
      const result = await loadCustomHome(id)
      if (request !== revision) return
      if (!result) throw new Error(t('customHome.notFound'))
      if (JSON.stringify(definition.value) !== JSON.stringify(result)) definition.value = result
    } catch (error) {
      if (request !== revision) return
      definition.value = null
      errorMessage.value = error instanceof Error ? error.message : String(error)
    } finally { if (request === revision) isLoading.value = false }
  }
  /** Refreshes only when the document becomes visible. */
  function onVisibility(): void { if (document.visibilityState === 'visible') void refresh() }
  /** Reloads after another window may have saved a definition. */
  function onFocus(): void { void refresh() }
  watch(() => toValue(homeId), () => { definition.value = null; void refresh() }, { immediate: true })
  window.addEventListener('focus', onFocus)
  window.addEventListener('custom-homes-changed', onFocus)
  document.addEventListener('visibilitychange', onVisibility)
  onScopeDispose(() => {
    revision++
    window.removeEventListener('focus', onFocus)
    window.removeEventListener('custom-homes-changed', onFocus)
    document.removeEventListener('visibilitychange', onVisibility)
  })
  return { definition, errorMessage, isLoading, refresh }
}