import { useStorage } from '@/services/storage'
import { loadCustomHome } from '@/services/customHomes'
import type { RouteLocationRaw } from 'vue-router'

/** @returns Configured landing route, falling back safely when the saved home is missing or unreadable. */
export async function resolveDefaultHomeRoute(): Promise<RouteLocationRaw> {
  const id = useStorage().getParameters().defaultHomeId.value
  if (id) {
    try {
      if (await loadCustomHome(id)) return { name: 'custom-home', params: { homeId: id } }
    } catch { /* Unavailable local storage must not prevent opening the aggregated home. */ }
  }
  return { name: 'home' }
}