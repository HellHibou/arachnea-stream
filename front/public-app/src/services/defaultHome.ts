import { loadCustomHome } from '@/services/customHomes'
import { loadDefaultHome } from '@/services/profileDefaultHome'
import type { RouteLocationRaw } from 'vue-router'
import { customHomeRoute } from '@/services/customHomeRoute'
import { reportPersonalDataFailure } from '@/services/personalDataStatus'

/** @returns The selected profile’s default home, or the all-sources overview page. */
export async function resolveDefaultHomeRoute(): Promise<RouteLocationRaw> {
  try {
    const id = await loadDefaultHome()
    if (id) { const home = await loadCustomHome(id); if (home) return customHomeRoute(home) }
  } catch (error) { reportPersonalDataFailure(error) }
  return { name: 'home' }
}
