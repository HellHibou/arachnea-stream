<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { useAdminApi } from '@/composables/useAdminApi'
import { useI18n } from '@/i18n'
import {
  lookupCredential,
  openExternalUrl,
  type AdminServiceEntry,
} from '@/services/adminApi'

const props = withDefaults(
  defineProps<{
    /** Service store (group) identifier owning the service. */
    serviceStoreId: string | null
    /** Service ID to show credentials for, or null to hide dialog. */
    serviceId: string | null
    /** Display label for the dialog title: service title with source code. */
    serviceTitle?: string | null
  }>(),
  {
    serviceStoreId: null,
    serviceTitle: null,
  },
)

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

/** Whether the password is currently displayed in clear text. */
const showPassword = ref(false)

/** Save is allowed only when both a login and a password are provided. */
const canSave = computed(() => login.value.trim().length > 0 && password.value.length > 0)

const service = ref<AdminServiceEntry | null>(null)

watch(
  () => props.serviceId,
  async (newId) => {
    if (newId && props.serviceStoreId) {
      await loadCredentials(props.serviceStoreId, newId)
    } else {
      resetForm()
    }
  },
)

async function loadCredentials(serviceStoreId: string, serviceId: string): Promise<void> {
  resetForm()
  const result = await getCredentials(serviceStoreId, serviceId)
  if (result?.credentials) {
    const cred = lookupCredential(result.credentials, serviceStoreId, serviceId)
    if (cred?.login_masked) {
      login.value = cred.login_masked
    }
    service.value = {
      id: serviceId,
      service_store_id: serviceStoreId,
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
  showPassword.value = false
  service.value = null
}

async function handleSave(): Promise<void> {
  const storeId = props.serviceStoreId
  const serviceId = props.serviceId
  if (!storeId || !serviceId) return
  localError.value = null
  successMessage.value = null

  const success = await setCredentials(storeId, serviceId, login.value, password.value)
  if (success) {
    successMessage.value = t('credentials.saved')
    password.value = ''
    emit('saved')
    handleClose()
  } else {
    localError.value = error.value?.message ?? t('error.unknown')
  }
}

async function handleClear(): Promise<void> {
  const storeId = props.serviceStoreId
  const serviceId = props.serviceId
  if (!storeId || !serviceId) return
  localError.value = null
  successMessage.value = null

  const success = await clearCredentials(storeId, serviceId)
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
    void openExternalUrl(service.value.credentials.signup_url)
  }
}
</script>

<template>
  <v-dialog
    :model-value="isOpen"
    max-width="640"
    @update:model-value="!$event && handleClose()"
  >
    <v-card v-if="service">
      <v-card-title class="credentials-dialog-title">
        {{ t('credentials.title', { service: serviceTitle ?? serviceId }) }}
      </v-card-title>

      <v-card-text>
        <v-alert
          v-if="service.credentials?.required"
          type="info"
          variant="tonal"
          class="mb-4"
        >
          <div>{{ t('credentials.required') }}</div>
          <div v-if="service.credentials?.signup_url" class="mt-2">
            <a
              href="#"
              @click.prevent="openSignupUrl"
            >
              {{ t('credentials.openSignup') }}
            </a>
          </div>
          <div v-if="service.credentials?.signup_url" class="mt-2 text-body-2">
            {{ t('credentials.signupPasswordWarning') }}
          </div>
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
            :type="showPassword ? 'text' : 'password'"
            variant="outlined"
            autocomplete="new-password"
            class="mt-2"
          >
            <template #append-inner>
              <v-icon
                :icon="showPassword ? 'mdi-eye-off' : 'mdi-eye'"
                :aria-label="showPassword ? t('credentials.hidePassword') : t('credentials.showPassword')"
                role="button"
                tabindex="0"
                @click="showPassword = !showPassword"
              />
            </template>
          </v-text-field>
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
          :disabled="isLoading || !canSave"
          @click="handleSave"
        >
          {{ t('credentials.save') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
/* Let long "title (source)" headings wrap instead of being ellipsized. */
.credentials-dialog-title {
  white-space: normal !important;
  overflow-wrap: anywhere;
}
</style>
