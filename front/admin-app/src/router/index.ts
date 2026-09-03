import { createRouter, createWebHistory } from 'vue-router'

import { getAppBasePath } from '@/services/baseUrl'
import { defaultServicesPath } from '@/services/appConfig'

const router = createRouter({
  history: createWebHistory(getAppBasePath()),
  routes: [
    {
      path: '/',
      name: 'home',
      redirect: '/services',
    },
    {
      path: '/services',
      // Redirects to the first configured service group; `/services/<id>`
      // below is the canonical parametrized route used for direct links.
      redirect: () => defaultServicesPath(),
    },
    {
      path: '/services/:serviceStoreId',
      name: 'services',
      component: () => import('@/views/ServicesView.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
      meta: { requiresAuth: false },
    },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('@/views/SettingsView.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/:pathMatch(.*)*',
      name: 'not-found',
      component: () => import('@/views/NotFoundView.vue'),
      meta: { requiresAuth: false },
    },
  ],
})

export default router
