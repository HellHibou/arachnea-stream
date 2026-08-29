import { createRouter, createWebHistory } from 'vue-router'

import { getAppBasePath } from '@/services/baseUrl'

const router = createRouter({
  history: createWebHistory(getAppBasePath()),
  routes: [
    {
      path: '/',
      name: 'home',
      redirect: { name: 'services' },
    },
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
      meta: { requiresAuth: false },
    },
    {
      path: '/services',
      name: 'services',
      component: () => import('@/views/ServicesView.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('@/views/SettingsView.vue'),
      meta: { requiresAuth: true },
    },
  ],
})

export default router
