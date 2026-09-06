<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'

import { useAdminApi } from '@/composables/useAdminApi'
import { useI18n } from '@/i18n'
import { getAppBasePath } from '@/services/baseUrl'
import type { SettingsResponse, SettingSource } from '@/services/adminApi'

const { t } = useI18n()
const route = useRoute()
const { getStatus, getSettings, updateSettings, setAdminPassword, isLoading, error } = useAdminApi()

/** Route name of the restricted popup variant rendered by the desktop window. */
const POPUP_ROUTE_NAME = 'settings-popup'

/**
 * Wait for the deferred application (1 s) and its bounded connection drain
 * (2 s) before navigating to the newly bound listener.
 */
const REDIRECT_DELAY_MS = 4000

const settings = ref<SettingsResponse | null>(null)
const passwordConfigured = ref(false)
const localError = ref<string | null>(null)
const successMessage = ref<string | null>(null)
const redirectTarget = ref<string | null>(null)

const port = ref<number | null>(null)
const networkMode = ref<string>('private')
const entrypointRoot = ref('')

const currentCountry = ref('')
const cacheMaxDiskBytes = ref<number | null>(null)
const cacheMaxMemoryBytes = ref<number | null>(null)

const currentPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const passwordError = ref<string | null>(null)
const passwordSuccess = ref<string | null>(null)

/**
 * Restricted popup variant: the full shell stays visible, but the "Server"
 * and "Password" cards are hidden and only the application settings are
 * submitted.
 */
const popupMode = computed(() => route.name === POPUP_ROUTE_NAME)

const isLocalModeAllowed = computed(() => {
  return ['localhost', '127.0.0.1', '::1'].includes(window.location.hostname)
})

const networkModes = computed(() => {
  const modes = [
    { value: 'private', title: t('settings.networkModePrivate') },
    { value: 'public', title: t('settings.networkModePublic') },
  ]

  return isLocalModeAllowed.value
    ? [{ value: 'local', title: t('settings.networkModeLocal') }, ...modes]
    : modes
})

function sourceLabel(source: SettingSource): string {
  switch (source) {
    case 'command_line':
      return t('settings.sourceCommandLine')
    case 'configuration':
      return t('settings.sourceConfiguration')
    default:
      return t('settings.sourceDefault')
  }
}

async function loadSettings(): Promise<void> {
  const [statusResult, settingsResult] = await Promise.all([getStatus(), getSettings()])
  if (statusResult) {
    passwordConfigured.value = statusResult.password_configured
  }
  if (settingsResult) {
    settings.value = settingsResult
    port.value = settingsResult.server_port
    networkMode.value = settingsResult.network_mode
    entrypointRoot.value = settingsResult.entrypoint_root ?? ''
    currentCountry.value = settingsResult.current_country ?? ''
    cacheMaxDiskBytes.value = settingsResult.cache_max_disk_bytes ?? null
    cacheMaxMemoryBytes.value = settingsResult.cache_max_memory_bytes ?? null
  }
}

/**
 * Schedules the navigation to the new administration URL once the running
 * server has re-bound its listener (port or entrypoint root change).
 */
function scheduleRedirect(target: string): void {
  redirectTarget.value = target
  setTimeout(() => {
    window.location.assign(target)
  }, REDIRECT_DELAY_MS)
}

/**
 * Returns whether the target admin URL differs from the one currently serving
 * this page (port or entrypoint root change).
 */
function redirectToAnotherBase(target: string): boolean {
  const normalized = new URL(target)
  normalized.pathname = `${normalized.pathname.replace(/\/+$/, '')}/`
  const current = new URL(window.location.origin + getAppBasePath())
  current.pathname = `${current.pathname.replace(/\/+$/, '')}/`
  return normalized.toString() !== current.toString()
}

async function handleSaveSettings(): Promise<void> {
  localError.value = null
  successMessage.value = null
  redirectTarget.value = null

  const country = currentCountry.value.trim().toUpperCase()
  if (country && !/^[A-Z]{2}$/.test(country)) {
    localError.value = t('settings.countryInvalid')
    return
  }
  for (const value of [cacheMaxDiskBytes.value, cacheMaxMemoryBytes.value]) {
    if (value !== null && (!Number.isInteger(value) || value < 0)) {
      localError.value = t('settings.cacheInvalid')
      return
    }
  }

  // The popup variant only submits the application settings; the full page
  // submits the server settings too (empty strings / zero clear overrides).
  const request = popupMode.value
    ? {
        current_country: country,
        cache_max_disk_bytes: cacheMaxDiskBytes.value ?? 0,
        cache_max_memory_bytes: cacheMaxMemoryBytes.value ?? 0,
      }
    : (() => {
        if (port.value && (port.value < 1 || port.value > 65535)) {
          localError.value = t('settings.portInvalid')
          return null
        }

        const root = entrypointRoot.value.trim()
        if (root && (root.startsWith('/') || root.split('/').some((segment) => !segment))) {
          localError.value = t('settings.rootInvalid')
          return null
        }

        return {
          server_port: port.value ?? undefined,
          network_mode: networkMode.value as 'local' | 'private' | 'public',
          // An explicit empty string tells the backend to clear the persisted root.
          entrypoint_root: root,
          current_country: country,
          cache_max_disk_bytes: cacheMaxDiskBytes.value ?? 0,
          cache_max_memory_bytes: cacheMaxMemoryBytes.value ?? 0,
        }
      })()

  if (request === null) {
    return
  }

  const result = await updateSettings(request)

  if (!result) {
    localError.value = error.value?.message ?? t('error.unknown')
    return
  }

  if (result.restart_required) {
    // Hot application unavailable (desktop mode or no REST handle).
    successMessage.value = t('settings.saved')
    await loadSettings()
    return
  }

  if (result.apply_error) {
    localError.value = result.apply_error
    await loadSettings()
    return
  }

  successMessage.value = t('settings.appliedHot')
  if (result.admin_url && redirectToAnotherBase(result.admin_url)) {
    scheduleRedirect(result.admin_url)
  } else {
    await loadSettings()
  }
}

async function handleChangePassword(): Promise<void> {
  passwordError.value = null
  passwordSuccess.value = null

  if (newPassword.value.length < 8) {
    passwordError.value = t('adminPassword.tooShort')
    return
  }

  if (newPassword.value !== confirmPassword.value) {
    passwordError.value = t('adminPassword.mismatch')
    return
  }

  const needsCurrent = passwordConfigured.value
  if (needsCurrent && !currentPassword.value) {
    passwordError.value = t('adminPassword.required')
    return
  }

  const success = await setAdminPassword(
    needsCurrent ? currentPassword.value : undefined,
    newPassword.value,
  )

  if (success) {
    passwordSuccess.value = t('adminPassword.saved')
    currentPassword.value = ''
    newPassword.value = ''
    confirmPassword.value = ''
    await loadSettings()
  } else {
    passwordError.value = error.value?.message ?? t('error.unknown')
  }
}

onMounted(() => {
  loadSettings()
})
</script>

<template>
  <v-container fluid>
    <v-row>
      <v-col cols="12" md="8" lg="6">
        <h1 class="text-h4 mb-2">{{ t('settings.title') }}</h1>
        <p class="text-body-1 text-medium-emphasis mb-4">
          {{ t('settings.description') }}
        </p>
        <v-alert v-if="localError" type="error" variant="tonal" class="mb-4" :text="localError" />
        <v-alert v-if="successMessage" type="success" variant="tonal" class="mb-4" :text="successMessage" />
        <v-alert v-if="redirectTarget" type="info" variant="tonal" class="mb-4" :text="t('settings.redirectNotice', { url: redirectTarget })" />
        <v-row class="mb-2">
          <v-spacer />
          <v-btn color="primary" variant="elevated" :loading="isLoading" @click="handleSaveSettings">
            {{ t('settings.save') }}
          </v-btn>
        </v-row>
        <v-card v-if="!popupMode" variant="outlined" class="mb-6">
          <v-card-title>{{ t('settings.serverGroup') }}</v-card-title>
          <v-card-text>
            <v-text-field v-model.number="port" :label="t('settings.serverPort')" :placeholder="t('settings.serverPortPlaceholder')" type="number" variant="outlined" :hint="settings ? t('settings.source', { source: sourceLabel(settings.server_port_source) }) : undefined" persistent-hint />
            <v-select v-model="networkMode" :label="t('settings.networkMode')" :items="networkModes" variant="outlined" class="mt-4" :hint="settings ? t('settings.source', { source: sourceLabel(settings.network_mode_source) }) : undefined" persistent-hint />
            <v-alert v-if="networkMode === 'public'" type="warning" variant="tonal" class="mt-2" :text="t('settings.publicHttpWarning')" />
            <v-alert v-if="networkMode === 'local'" type="info" variant="tonal" class="mt-2" :text="t('settings.localModeWarning')" />
            <v-text-field v-model="entrypointRoot" :label="t('settings.entrypointRoot')" :placeholder="t('settings.entrypointRootPlaceholder')" variant="outlined" class="mt-4" :hint="settings ? t('settings.source', { source: sourceLabel(settings.entrypoint_root_source) }) : undefined" persistent-hint />
          </v-card-text>
        </v-card>
        <v-card variant="outlined" class="mb-6">
          <v-card-title>{{ t('settings.applicationGroup') }}</v-card-title>
          <v-card-text>
            <v-text-field v-model="currentCountry" :label="t('settings.currentCountry')" :placeholder="t('settings.currentCountryPlaceholder')" variant="outlined" :hint="settings ? t('settings.source', { source: sourceLabel(settings.current_country_source) }) : undefined" persistent-hint />
            <v-text-field v-model.number="cacheMaxDiskBytes" :label="t('settings.cacheMaxDiskBytes')" type="number" variant="outlined" class="mt-4" :hint="t('settings.cacheBytesHint')" persistent-hint />
            <v-text-field v-model.number="cacheMaxMemoryBytes" :label="t('settings.cacheMaxMemoryBytes')" type="number" variant="outlined" class="mt-4" :hint="t('settings.cacheBytesHint')" persistent-hint />
          </v-card-text>
        </v-card>
        <v-card v-if="!popupMode" variant="outlined">
          <v-card-title>{{ t('adminPassword.title') }}</v-card-title>
          <v-card-text>
            <v-alert v-if="passwordError" type="error" variant="tonal" class="mb-4" :text="passwordError" />
            <v-alert v-if="passwordSuccess" type="success" variant="tonal" class="mb-4" :text="passwordSuccess" />
            <v-form @submit.prevent="handleChangePassword">
              <v-text-field v-if="passwordConfigured" v-model="currentPassword" :label="t('adminPassword.currentPassword')" :placeholder="t('adminPassword.currentPasswordPlaceholder')" type="password" variant="outlined" autocomplete="current-password" />
              <v-text-field v-model="newPassword" :label="t('adminPassword.newPassword')" :placeholder="t('adminPassword.newPasswordPlaceholder')" type="password" variant="outlined" autocomplete="new-password" class="mt-2" />
              <v-text-field v-model="confirmPassword" :label="t('adminPassword.confirmPassword')" :placeholder="t('adminPassword.confirmPasswordPlaceholder')" type="password" variant="outlined" autocomplete="new-password" class="mt-2" />
            </v-form>
          </v-card-text>
          <v-card-actions>
            <v-spacer />
            <v-btn color="primary" variant="elevated" :loading="isLoading" @click="handleChangePassword">
              {{ t('adminPassword.save') }}
            </v-btn>
          </v-card-actions>
        </v-card>
      </v-col>
    </v-row>
  </v-container>
</template>
