import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'

/** Application route definitions. */
const routes: RouteRecordRaw[] = [
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
    path: '/categorie/:categoryKey',
    name: 'category',
    component: () => import('@/views/CategoryView.vue'),
    props: (route) => ({
      categoryKey: readRouteParam(route.params.categoryKey),
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
  history: createWebHistory(),
  routes,
  scrollBehavior(_to, _from, savedPosition) {
    return savedPosition ?? { top: 0, left: 0 }
  },
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
