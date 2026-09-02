<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useTheme } from 'vuetify'

import { useAdminApi } from '@/composables/useAdminApi'
import { useErrorNotifications } from '@/composables/useErrorNotifications'
import { useI18n, setLanguage } from '@/i18n'
import { loadStoredTheme, persistTheme, findThemePreset } from '@/services/theme'
import type { ThemeMode } from '@/services/theme'
import type { StatusResponse } from '@/services/adminApi'
import ErrorNotificationStack from '@/components/errors/ErrorNotificationStack.vue'

const router = useRouter()
const vuetifyTheme = useTheme()
const { t, availableLanguages, selectedLanguage, resolvedLanguage } = useI18n()
const { getStatus, logout } = useAdminApi()
const { notifications: errorNotifications, dismiss: dismissError } = useErrorNotifications()

const isLoading = ref(true)
const isAuthenticated = ref(false)
const authRequired = ref(false)
const status = ref<StatusResponse | null>(null)
const theme = ref<ThemeMode>(loadStoredTheme())

const isDark = computed({
  get: () => findThemePreset(theme.value).variant === 'dark',
  set: (value: boolean) => {
    theme.value = value ? 'dark' : 'light'
    persistTheme(theme.value)
  },
})

async function checkAuth(): Promise<void> {
  isLoading.value = true
  const result = await getStatus()
  if (result) {
    status.value = result
    authRequired.value = result.auth_required
    isAuthenticated.value = result.authenticated

    if (router.currentRoute.value.name === 'not-found') {
      // Keep unknown URLs on the localized error page, even when authentication is required.
    } else if (result.auth_required && !result.authenticated) {
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

watch(theme, (newTheme) => {
  const preset = findThemePreset(newTheme)
  vuetifyTheme.change(preset.variant)
})

watch(selectedLanguage, (newLang) => {
  if (newLang) {
    setLanguage(newLang)
  }
})

watch(
  () => router.currentRoute.value.name,
  () => {
    void checkAuth()
  },
)

onMounted(() => {
  // Apply the stored theme to Vuetify on startup
  vuetifyTheme.change(findThemePreset(theme.value).variant)
  checkAuth()
})
</script>

<template>
  <v-app>
    <v-app-bar v-if="isAuthenticated" color="primary" density="compact">
      <v-app-bar-title>{{ t('app.title') }}</v-app-bar-title>

      <v-spacer />

      <!-- Dark/Light theme switch -->
      <v-tooltip :text="isDark ? t('theme.switchToLight') : t('theme.switchToDark')" location="bottom">
        <template #activator="{ props: tooltipProps }">
          <button
            v-bind="tooltipProps"
            type="button"
            class="theme-switch"
            :class="{ 'theme-switch--dark': isDark }"
            role="switch"
            :aria-checked="isDark"
            :aria-label="isDark ? t('theme.switchToLight') : t('theme.switchToDark')"
            @click="isDark = !isDark"
          >
            <span class="theme-switch__track">
              <span class="theme-switch__thumb">
                <v-icon size="14" class="theme-switch__icon">
                  {{ isDark ? 'mdi-moon-waning-crescent' : 'mdi-white-balance-sunny' }}
                </v-icon>
              </span>
            </span>
          </button>
        </template>
      </v-tooltip>

      <!-- Language selector -->
      <v-select
        :model-value="resolvedLanguage"
        :items="availableLanguages"
        item-title="labelLocal"
        item-value="code"
        variant="outlined"
        density="compact"
        hide-details
        class="mr-2"
        style="max-width: 150px"
        @update:model-value="setLanguage($event as string)"
      >
        <template #selection="{ item }">
          <span>{{ item.flag }} {{ item.labelLocal }}</span>
        </template>
        <template #item="{ item, props: itemProps }">
          <v-list-item v-bind="itemProps">
            <template #title>
              <span>{{ item.flag }} {{ item.labelLocal }}</span>
            </template>
          </v-list-item>
        </template>
      </v-select>

      <v-btn
        v-if="authRequired"
        icon="mdi-logout"
        :title="t('nav.logout')"
        @click="handleLogout"
      />
    </v-app-bar>

    <v-navigation-drawer v-if="isAuthenticated">
      <v-list nav>
        <v-list-item :to="{ name: 'services' }" prepend-icon="mdi-server" :title="t('nav.services')" />
        <v-list-item :to="{ name: 'settings' }" prepend-icon="mdi-cog" :title="t('nav.settings')" />
      </v-list>
    </v-navigation-drawer>

    <v-main>
      <v-progress-linear
        v-if="isLoading"
        indeterminate
        color="primary"
      />
      <router-view v-else />
    </v-main>

    <ErrorNotificationStack
      :notifications="errorNotifications"
      @dismiss="dismissError"
    />
  </v-app>
</template>

<style scoped>
/* Theme toggle switch */
.theme-switch {
  position: relative;
  width: 57px;
  height: 31px;
  padding: 0;
  margin-right: 16px;
  border: none;
  background: transparent;
  cursor: pointer;
}

.theme-switch__track {
  display: flex;
  align-items: center;
  width: 100%;
  height: 100%;
  background-color: rgba(255, 255, 255, 0.3);
  border-radius: 16px;
  transition: background-color 0.2s ease;
}

.theme-switch--dark .theme-switch__track {
  background-color: rgba(255, 255, 255, 0.5);
}

.theme-switch__thumb {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 26px;
  height: 26px;
  background-color: #fff;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.2s ease;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.2);
}

.theme-switch--dark .theme-switch__thumb {
  transform: translateX(26px);
  background-color: #1a237e;
}

.theme-switch__icon {
  color: #ffa000;
}
</style>
