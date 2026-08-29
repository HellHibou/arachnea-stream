<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'

import { useAdminApi } from '@/composables/useAdminApi'
import { useI18n, setLanguage } from '@/i18n'
import { loadStoredTheme, persistTheme, findThemePreset } from '@/services/theme'
import type { ThemeMode } from '@/services/theme'
import type { StatusResponse } from '@/services/adminApi'

const router = useRouter()
const { t, availableLanguages, selectedLanguage, resolvedLanguage } = useI18n()
const { getStatus, logout } = useAdminApi()

const isLoading = ref(true)
const isAuthenticated = ref(false)
const authRequired = ref(false)
const status = ref<StatusResponse | null>(null)
const theme = ref<ThemeMode>(loadStoredTheme())

const themeIcon = computed(() => {
  const preset = findThemePreset(theme.value)
  return preset.key === 'dark' ? 'mdi-weather-night' : preset.key === 'light' ? 'mdi-weather-sunny' : 'mdi-monitor'
})

const themeLabel = computed(() => t(`theme.${theme.value}`))

async function checkAuth(): Promise<void> {
  isLoading.value = true
  const result = await getStatus()
  if (result) {
    status.value = result
    authRequired.value = result.auth_required
    isAuthenticated.value = result.authenticated

    if (result.auth_required && !result.authenticated) {
      router.push({ name: 'login' })
    } else if (router.currentRoute.value.name === 'login') {
      router.push({ name: 'services' })
    }
  }
  isLoading.value = false
}

async function handleLogout(): Promise<void> {
  await logout()
  isAuthenticated.value = false
  router.push({ name: 'login' })
}

function cycleTheme(): void {
  const modes: ThemeMode[] = ['system', 'light', 'dark']
  const currentIndex = modes.indexOf(theme.value)
  const nextIndex = (currentIndex + 1) % modes.length
  theme.value = modes[nextIndex]!
  persistTheme(theme.value)
}

watch(theme, (newTheme) => {
  const preset = findThemePreset(newTheme)
  document.documentElement.setAttribute('data-theme', preset.variant)
})

watch(selectedLanguage, (newLang) => {
  if (newLang) {
    setLanguage(newLang)
  }
})

onMounted(() => {
  checkAuth()
})
</script>

<template>
  <v-app>
    <v-app-bar v-if="isAuthenticated && !authRequired || isAuthenticated" color="primary" density="compact">
      <v-app-bar-title>{{ t('app.title') }}</v-app-bar-title>

      <v-tabs>
        <v-tab :to="{ name: 'services' }" prepend-icon="mdi-server">
          {{ t('nav.services') }}
        </v-tab>
        <v-tab :to="{ name: 'settings' }" prepend-icon="mdi-cog">
          {{ t('nav.settings') }}
        </v-tab>
      </v-tabs>

      <v-spacer />

      <v-btn
        :icon="themeIcon"
        :title="themeLabel"
        @click="cycleTheme"
      />

      <v-menu>
        <template #activator="{ props }">
          <v-btn v-bind="props" icon="mdi-translate" />
        </template>
        <v-list>
          <v-list-item
            v-for="lang in availableLanguages"
            :key="lang.code"
            :active="resolvedLanguage === lang.code"
            @click="setLanguage(lang.code)"
          >
            <v-list-item-title>
              {{ lang.flag }} {{ lang.labelLocal }}
            </v-list-item-title>
          </v-list-item>
        </v-list>
      </v-menu>

      <v-btn
        v-if="authRequired"
        icon="mdi-logout"
        :title="t('nav.logout')"
        @click="handleLogout"
      />
    </v-app-bar>

    <v-main>
      <v-progress-linear
        v-if="isLoading"
        indeterminate
        color="primary"
      />
      <router-view v-else />
    </v-main>
  </v-app>
</template>
