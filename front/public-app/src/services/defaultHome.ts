import { useStorage } from '@/services/storage'
import { loadCustomHome } from '@/services/customHomes'
import { useServiceMetadata } from '@/composables/useServiceMetadata'
import type { RouteLocationRaw } from 'vue-router'
import { customHomeRoute } from '@/services/customHomeRoute'

/** @returns Configured landing route, falling back safely when the saved home is missing or unreadable. */
export async function resolveDefaultHomeRoute(): Promise<RouteLocationRaw> {
  const id = useStorage().getParameters().defaultHomeId.value
  if (id) {
    try {
      const home = await loadCustomHome(id)
      if (home) return customHomeRoute(home)
      if (id.startsWith('service:')) {
        const sourceId = id.slice('service:'.length)
        const services = await useServiceMetadata().load()
        if (services.some((service) => service.id === sourceId && service.supportedQueries.includes('load_home'))) {
          return { name: 'source-home', params: { sourceId } }
        }
      }
    } catch { /* Unavailable local storage must not prevent opening the aggregated home. */ }
  }
  return { name: 'home' }
}