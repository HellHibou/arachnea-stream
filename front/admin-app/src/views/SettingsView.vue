<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

import { useAdminApi } from '@/composables/useAdminApi'
import { useI18n } from '@/i18n'
import type { SettingsResponse, SettingSource } from '@/services/adminApi'

const { t } = useI18n()
const { getStatus, getSettings, updateSettings, setAdminPassword, isLoading, error } = useAdminApi()

const settings = ref<SettingsResponse | null>(null)
const passwordConfigured = ref(false)
const localError = ref<string | null>(null)
const successMessage = ref<string | null>(null)

const port = ref<number | null>(null)
const networkMode = ref<string>('private')
const entrypointRoot = ref('')

const currentPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const passwordError = ref<string | null>(null)
const passwordSuccess = ref<string | null>(null)

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
  }
}

async function handleSaveSettings(): Promise<void> {
  localError.value = null
  successMessage.value = null

  if (port.value && (port.value < 1 || port.value > 65535)) {
    localError.value = t('settings.portInvalid')
    return
  }

  if (entrypointRoot.value && !entrypointRoot.value.startsWith('/')) {
    localError.value = t('settings.rootInvalid')
    return
  }

  const result = await updateSettings({
    server_port: port.value ?? undefined,
    network_mode: networkMode.value as 'local' | 'private' | 'public',
    entrypoint_root: entrypointRoot.value || undefined,
  })

  if (result) {
    successMessage.value = t('settings.saved')
    await loadSettings()
  } else {
    localError.value = error.value?.message ?? t('error.unknown')
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
        <v-card variant="outlined" class="mb-6">
          <v-card-title>{{ t('settings.title') }}</v-card-title>
          <v-card-text>
            <v-text-field v-model.number="port" :label="t('settings.serverPort')" :placeholder="t('settings.serverPortPlaceholder')" type="number" variant="outlined" :hint="settings ? t('settings.source', { source: sourceLabel(settings.server_port_source) }) : undefined" persistent-hint />
            <v-select v-model="networkMode" :label="t('settings.networkMode')" :items="networkModes" variant="outlined" class="mt-4" :hint="settings ? t('settings.source', { source: sourceLabel(settings.network_mode_source) }) : undefined" persistent-hint />
            <v-alert v-if="networkMode === 'public'" type="warning" variant="tonal" class="mt-2" :text="t('settings.publicHttpWarning')" />
            <v-alert v-if="networkMode === 'local'" type="info" variant="tonal" class="mt-2" :text="t('settings.localModeWarning')" />
            <v-text-field v-model="entrypointRoot" :label="t('settings.entrypointRoot')" :placeholder="t('settings.entrypointRootPlaceholder')" variant="outlined" class="mt-4" :hint="settings ? t('settings.source', { source: sourceLabel(settings.entrypoint_root_source) }) : undefined" persistent-hint />
          </v-card-text>
          <v-card-actions>
            <v-spacer />
            <v-btn color="primary" variant="elevated" :loading="isLoading" @click="handleSaveSettings">
              {{ t('settings.save') }}
            </v-btn>
          </v-card-actions>
        </v-card>
        <v-card variant="outlined">
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
