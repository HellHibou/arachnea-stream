import { onScopeDispose, shallowRef, toValue, watch, type MaybeRefOrGetter } from 'vue'
import { loadCustomHome } from '@/services/customHomes'
import { onWindowForeground } from '@/services/windowForeground'
import type { CustomHomeDefinition } from '@/types/customHome'
import { t } from '@/i18n'

/**
 * Reads persisted definitions and refreshes on focus/visibility across browser/native views.
 * @param homeId Stable route id.
 * @param resolvedHome Reads the definition already fetched by the route guard, when available.
 * @returns Current definition, read failure and refresh action guarded against stale reads.
 */
export function useCustomHomeDefinition(homeId: MaybeRefOrGetter<string>, resolvedHome: () => CustomHomeDefinition | undefined) {
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
  watch([() => toValue(homeId), resolvedHome], ([id, resolved]) => {
    revision++
    errorMessage.value = null
    isLoading.value = false
    definition.value = resolved?.id === id ? resolved : null
    if (id && !definition.value) void refresh()
  }, { immediate: true })
  const stopForeground = onWindowForeground(() => { void refresh() })
  window.addEventListener('custom-homes-changed', refresh)
  onScopeDispose(() => {
    revision++
    stopForeground()
    window.removeEventListener('custom-homes-changed', refresh)
  })
  return { definition, errorMessage, isLoading, refresh }
}
