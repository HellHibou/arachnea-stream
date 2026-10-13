import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'

import { getAppBasePath } from '@/services/baseUrl'
import { resolveDefaultHomeRoute } from '@/services/defaultHome'
import { listCustomHomes, loadCustomHome } from '@/services/customHomes'
import { customHomeRoute, customHomeSlug } from '@/services/customHomeRoute'

/** Application route definitions. */
const routes: RouteRecordRaw[] = [
  {
    path: '/home/:homeSlug',
    name: 'custom-home',
    component: () => import('@/views/CustomHomeView.vue'),
    props: (route) => ({ homeId: String(route.meta.homeId ?? ''), routeError: route.meta.homeRouteError ?? null }),
  },
  {
    path: '/homes/:homeId',
    name: 'legacy-custom-home',
    component: () => import('@/views/CustomHomeView.vue'),
    props: (route) => ({ homeId: readRouteParam(route.params.homeId) }),
  },
  {
    path: '/source/:sourceId',
    name: 'source-home',
    component: () => import('@/views/HomeView.vue'),
    props: (route) => ({ sourceId: readRouteParam(route.params.sourceId) }),
  },
  {
    path: '/',
    name: 'home',
    component: () => import('@/views/HomeView.vue'),
  },
  {
    path: '/search',
    name: 'search',
    component: () => import('@/views/SearchView.vue'),
  },
  {
    path: '/categorie/:categoryToken',
    name: 'category',
    component: () => import('@/views/CategoryView.vue'),
    props: (route) => ({
      categoryToken: readRouteParam(route.params.categoryToken),
    }),
  },
  {
    path: '/entry/:encodedEntry',
    name: 'entry-details',
    component: () => import('@/views/EntryDetailsView.vue'),
    props: (route) => ({
      encodedEntry: readRouteParam(route.params.encodedEntry),
    }),
  },
  {
    path: '/lives',
    name: 'lives',
    component: () => import('@/views/LivesView.vue'),
  },
  {
    path: '/lives/:encodedLive',
    name: 'live-details',
    component: () => import('@/views/LivesView.vue'),
    props: (route) => ({
      encodedLive: readRouteParam(route.params.encodedLive),
    }),
  },
  {
    path: '/:pathMatch(.*)*',
    name: 'not-found',
    component: () => import('@/views/NotFoundView.vue'),
  },
]

/** Vue Router instance with scroll behavior reset on navigation. */
const router = createRouter({
  history: createWebHistory(getAppBasePath()),
  routes,
  scrollBehavior(_to, _from, savedPosition) {
    return savedPosition ?? { top: 0, left: 0 }
  },
})

router.beforeEach(async (to) => {
  if (to.name === 'legacy-custom-home') {
    const home = await loadCustomHome(readRouteParam(to.params.homeId))
    if (home) return { ...customHomeRoute(home), query: to.query, hash: to.hash, replace: true }
  }
  if (to.name === 'custom-home') {
    const slug = readRouteParam(to.params.homeSlug)
    const { homes } = await listCustomHomes()
    const matches = homes.filter((home) => customHomeSlug(home.name) === slug)
    to.meta.homeId = matches.length === 1 ? matches[0]!.id : ''
    to.meta.homeRouteError = matches.length === 1 ? null : matches.length
      ? 'Multiple accueils share this URL; rename them in management' : 'Accueil not found'
  }
  if (to.name === 'home') {
    const target = await resolveDefaultHomeRoute()
    if (typeof target === 'object' && 'name' in target && target.name !== 'home') return { ...target, query: to.query, hash: to.hash }
  }
})

/**
 * Extracts a string route parameter from a potentially array-valued parameter.
 *
 * @param value - Route parameter value which may be a string, string array, or undefined.
 * @returns The first string value, or empty string if not available.
 */
function readRouteParam(value: string | string[] | undefined): string {
  return Array.isArray(value) ? value[0] ?? '' : value ?? ''
}

export default router
