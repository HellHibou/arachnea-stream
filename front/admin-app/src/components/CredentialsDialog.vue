<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { useAdminApi } from '@/composables/useAdminApi'
import { useI18n } from '@/i18n'
import type { AdminServiceEntry } from '@/services/adminApi'

const props = defineProps<{
  /** Service ID to show credentials for, or null to hide dialog. */
  serviceId: string | null
}>()

const emit = defineEmits<{
  close: []
  saved: []
}>()

const { t } = useI18n()
const { getCredentials, setCredentials, clearCredentials, isLoading, error } = useAdminApi()

const login = ref('')
const password = ref('')
const successMessage = ref<string | null>(null)
const localError = ref<string | null>(null)

const isOpen = computed(() => props.serviceId !== null)

const service = ref<AdminServiceEntry | null>(null)

watch(
  () => props.serviceId,
  async (newId) => {
    if (newId) {
      await loadCredentials(newId)
    } else {
      resetForm()
    }
  },
)

async function loadCredentials(serviceId: string): Promise<void> {
  resetForm()
  const result = await getCredentials(serviceId)
  if (result?.credentials && result.credentials[serviceId]) {
    const cred = result.credentials[serviceId]
    if (cred?.login_masked) {
      login.value = cred.login_masked
    }
    service.value = {
      id: serviceId,
      default_enabled: true,
      enabled: true,
      has_override: false,
      unavailable: false,
      credentials: cred,
    }
  }
}

function resetForm(): void {
  login.value = ''
  password.value = ''
  successMessage.value = null
  localError.value = null
  service.value = null
}

async function handleSave(): Promise<void> {
  if (!props.serviceId) return
  localError.value = null
  successMessage.value = null

  const success = await setCredentials(props.serviceId, login.value, password.value)
  if (success) {
    successMessage.value = t('credentials.saved')
    password.value = ''
    emit('saved')
  } else {
    localError.value = error.value?.message ?? t('error.unknown')
  }
}

async function handleClear(): Promise<void> {
  if (!props.serviceId) return
  localError.value = null
  successMessage.value = null

  const success = await clearCredentials(props.serviceId)
  if (success) {
    successMessage.value = t('credentials.cleared')
    login.value = ''
    password.value = ''
    emit('saved')
  } else {
    localError.value = error.value?.message ?? t('error.unknown')
  }
}

function handleClose(): void {
  emit('close')
}

function openSignupUrl(): void {
  if (service.value?.credentials?.signup_url) {
    window.open(service.value.credentials.signup_url, '_blank', 'noopener,noreferrer')
  }
}
</script>

<template>
  <v-dialog
    :model-value="isOpen"
    max-width="500"
    @update:model-value="!$event && handleClose()"
  >
    <v-card v-if="service">
      <v-card-title>
        {{ t('credentials.title', { service: service.title ?? service.id }) }}
      </v-card-title>

      <v-card-text>
        <v-alert
          v-if="service.credentials?.required"
          type="info"
          variant="tonal"
          class="mb-4"
        >
          {{ t('credentials.required') }}
          <a
            v-if="service.credentials.signup_url"
            href="#"
            @click.prevent="openSignupUrl"
          >
            {{ t('credentials.openSignup') }}
          </a>
        </v-alert>

        <v-alert
          v-if="localError"
          type="error"
          variant="tonal"
          class="mb-4"
          :text="localError"
        />

        <v-alert
          v-if="successMessage"
          type="success"
          variant="tonal"
          class="mb-4"
          :text="successMessage"
        />

        <v-form @submit.prevent="handleSave">
          <v-text-field
            v-model="login"
            :label="t('credentials.login')"
            :placeholder="t('credentials.loginPlaceholder')"
            variant="outlined"
            autocomplete="username"
          />

          <v-text-field
            v-model="password"
            :label="t('credentials.password')"
            :placeholder="t('credentials.passwordPlaceholder')"
            type="password"
            variant="outlined"
            autocomplete="new-password"
            class="mt-2"
          />
        </v-form>
      </v-card-text>

      <v-card-actions>
        <v-btn
          v-if="service.credentials?.configured"
          color="error"
          variant="text"
          :disabled="isLoading"
          @click="handleClear"
        >
          {{ t('credentials.clear') }}
        </v-btn>

        <v-spacer />

        <v-btn
          variant="text"
          :disabled="isLoading"
          @click="handleClose"
        >
          {{ t('credentials.cancel') }}
        </v-btn>

        <v-btn
          color="primary"
          variant="elevated"
          :loading="isLoading"
          @click="handleSave"
        >
          {{ t('credentials.save') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>
