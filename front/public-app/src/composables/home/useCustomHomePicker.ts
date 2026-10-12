import { onScopeDispose, shallowRef } from 'vue'
import { loadHomeSourceDiscovery } from '@/services/rustify'
import type { HomeSourceDiscovery } from '@/types/customHome'

/** @returns Explicit selected-source discovery, guarded against stale selections and disposal. */
export function useCustomHomePicker() {
  const discovery = shallowRef<HomeSourceDiscovery | null>(null)
  const isLoading = shallowRef(false)
  const errorMessage = shallowRef<string | null>(null)
  let revision = 0
  /** @param sourceId Exact user-selected source; empty clears without contacting providers. */
  async function discover(sourceId: string): Promise<void> {
    const request = ++revision
    discovery.value = null
    errorMessage.value = null
    isLoading.value = Boolean(sourceId)
    if (!sourceId) return
    try {
      const rows = await loadHomeSourceDiscovery([sourceId])
      if (request !== revision) return
      const row = rows[0]
      if (!row?.available) throw new Error(`Source unavailable: ${sourceId}`)
      discovery.value = row
    } catch (error) {
      if (request === revision) errorMessage.value = error instanceof Error ? error.message : String(error)
    } finally { if (request === revision) isLoading.value = false }
  }
  onScopeDispose(() => { revision++ })
  return { discovery, isLoading, errorMessage, discover }
}