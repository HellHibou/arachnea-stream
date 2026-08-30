<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

import { useAdminApi } from '@/composables/useAdminApi'
import { useI18n } from '@/i18n'
import type { AdminServiceEntry } from '@/services/adminApi'
import CredentialsDialog from '@/components/CredentialsDialog.vue'

const { t } = useI18n()
const {
  getServices,
  setServiceEnabled,
  resetServiceEnabled,
  reload,
  isLoading,
  error,
} = useAdminApi()

const services = ref<AdminServiceEntry[]>([])
const needsReload = ref(false)
const reloadResult = ref<string | null>(null)
const activeDialog = ref<string | null>(null)
/** Service awaiting activation after credentials are saved (null otherwise). */
const pendingActivation = ref<string | null>(null)
/** Service for which the "credentials required" confirmation dialog is open. */
const enableConfirmServiceId = ref<string | null>(null)
/** Bumped to force switch re-creation so cancelled toggles revert visually. */
const switchEpoch = ref(0)

const servicesWithCredentials = computed(() =>
  services.value.filter((s) => s.credentials?.required),
)

/** Catalog entry currently opened in the credentials dialog. */
const activeService = computed(() =>
  services.value.find((s) => s.id === activeDialog.value) ?? null,
)

/** Dialog heading: service title followed by its source code in parentheses. */
const activeDialogTitle = computed(() => {
  const service = activeService.value
  if (!service) {
    return null
  }
  return service.title ? `${service.title} (${service.id})` : service.id
})

/** Confirmation heading: required-credentials message with service and source. */
const enableConfirmDialogTitle = computed(() => {
  const service = services.value.find((s) => s.id === enableConfirmServiceId.value)
  const label = service
    ? (service.title ? `${service.title} (${service.id})` : service.id)
    : ''
  return t('services.enableWarning.title', { service: label })
})

async function loadServices(): Promise<void> {
  const result = await getServices()
  if (result) {
    services.value = result.services
  }
}

async function handleSetEnabled(serviceId: string, enabled: boolean): Promise<void> {
  const result = await setServiceEnabled(serviceId, enabled)
  if (result?.reload_required) {
    needsReload.value = true
    reloadResult.value = t('services.reloadRequired')
  }
  if (!result) {
    // The toggle failed: force the switch back to the persisted state.
    switchEpoch.value += 1
  }
  await loadServices()
}

/**
 * Intercepts switch activation: when the service requires credentials and
 * none are configured, ask the user how to proceed before enabling.
 */
function requestEnable(serviceId: string): void {
  const service = services.value.find((s) => s.id === serviceId)
  if (service?.credentials?.required && !service.credentials?.configured) {
    enableConfirmServiceId.value = serviceId
    return
  }
  void handleSetEnabled(serviceId, true)
}

/** Leaves the service disabled (user dismissed the confirmation dialog). */
async function cancelEnable(): Promise<void> {
  enableConfirmServiceId.value = null
  switchEpoch.value += 1
  await loadServices()
}

/** Opens the credentials dialog; activation resumes after a successful save. */
function enableWithCredentials(): void {
  const serviceId = enableConfirmServiceId.value
  enableConfirmServiceId.value = null
  if (!serviceId) {
    return
  }
  pendingActivation.value = serviceId
  activeDialog.value = serviceId
}

/** Activates the service without configuring credentials. */
function enableWithoutCredentials(): void {
  const serviceId = enableConfirmServiceId.value
  enableConfirmServiceId.value = null
  if (!serviceId) {
    return
  }
  void handleSetEnabled(serviceId, true)
}

/** Activates the pending service after credentials were saved. */
async function onCredentialsSaved(): Promise<void> {
  const serviceId = pendingActivation.value
  pendingActivation.value = null
  if (serviceId) {
    await handleSetEnabled(serviceId, true)
    return
  }
  await loadServices()
}

/** Drops the pending activation when the credentials dialog is cancelled. */
async function onCredentialsClosed(): Promise<void> {
  if (pendingActivation.value) {
    pendingActivation.value = null
    switchEpoch.value += 1
    await loadServices()
  }
  closeCredentials()
}

async function handleResetEnabled(serviceId: string): Promise<void> {
  const result = await resetServiceEnabled(serviceId)
  if (result?.reload_required) {
    needsReload.value = true
    reloadResult.value = t('services.reloadRequired')
  }
  await loadServices()
}

async function handleReload(): Promise<void> {
  const result = await reload()
  if (result?.applied) {
    needsReload.value = false
    reloadResult.value = t('reload.success')
  } else {
    reloadResult.value = result?.build_error ?? t('reload.failed')
  }
  await loadServices()
}

function openCredentials(serviceId: string): void {
  activeDialog.value = serviceId
}

function closeCredentials(): void {
  activeDialog.value = null
}

function getServiceName(service: AdminServiceEntry): string {
  return service.title ? `${service.title} (${service.id})` : service.id
}

/** Key-button color: green when credentials exist, red when an active service lacks them. */
function getCredentialsColor(service: AdminServiceEntry): string | undefined {
  if (service.credentials?.configured) {
    return 'success'
  }
  if (service.enabled) {
    return 'error'
  }
  return undefined
}

/** Key-button tooltip showing whether credentials are defined or absent. */
function getCredentialsTitle(service: AdminServiceEntry): string {
  return t('services.credentialsState', {
    state: service.credentials?.configured
      ? t('services.credentialsDefined')
      : t('services.credentialsAbsent'),
  })
}

onMounted(() => {
  loadServices()
})
</script>

<template>
  <v-container fluid>
    <v-row>
      <v-col cols="12">
        <h1 class="text-h4 mb-2">{{ t('services.title') }}</h1>
        <p class="text-body-1 text-medium-emphasis mb-4">
          {{ t('services.description') }}
        </p>

        <v-alert
          v-if="error"
          type="error"
          variant="tonal"
          class="mb-4"
          :text="error.message"
        />

        <v-alert
          v-if="needsReload"
          type="warning"
          variant="tonal"
          class="mb-4"
        >
          <div class="d-flex align-center justify-space-between">
            <span>{{ reloadResult }}</span>
            <v-btn
              color="warning"
              variant="elevated"
              size="small"
              :loading="isLoading"
              @click="handleReload"
            >
              {{ t('reload.button') }}
            </v-btn>
          </div>
        </v-alert>

        <v-alert
          v-else-if="reloadResult"
          type="success"
          variant="tonal"
          class="mb-4"
          :text="reloadResult"
        />

        <v-progress-circular
          v-if="isLoading && services.length === 0"
          indeterminate
          color="primary"
        />

        <v-card v-else-if="services.length === 0" variant="outlined">
          <v-card-text class="text-center text-medium-emphasis">
            {{ t('services.empty') }}
          </v-card-text>
        </v-card>

        <v-list v-else class="align-start">
          <v-list-item
            v-for="service in services"
            :key="service.id"
            :title="getServiceName(service)"
            :subtitle="service.description"
          >
            <template #prepend>
              <div class="d-flex align-start" style="gap: 10px;">
                <v-switch
                  :key="`${service.id}-${switchEpoch}`"
                  :model-value="service.enabled"
                  color="success"
                  hide-details
                  density="compact"
                  :disabled="service.unavailable"
                  @update:model-value="$event === true ? requestEnable(service.id) : handleSetEnabled(service.id, false)"
                />
                <div v-if="service.logo" class="service-logo">
                  <v-img :src="service.logo" :alt="service.title ?? service.id" contain />
                </div>
                <div v-else class="service-logo service-fallback">
                  <span v-if="service.title" >
                    {{ service.title.charAt(0).toUpperCase() }}
                  </span>
                  <span v-else >
                    {{ t('services.fallbackLogo') }}
                  </span>
                </div>
              </div>
            </template>

            <template #append>
              <div class="d-flex flex-column align-center" style="gap: 6px;">
                <v-chip
                  v-if="service.unavailable"
                  color="error"
                  size="small"
                  variant="tonal"
                >
                  {{ t('services.unavailable') }}
                </v-chip>
                <v-chip
                  v-else-if="service.enabled"
                  color="success"
                  size="small"
                  variant="tonal"
                >
                  {{ t('services.enabled') }}
                </v-chip>
                <v-chip
                  v-else
                  color="default"
                  size="small"
                  variant="tonal"
                >
                  {{ t('services.disabled') }}
                </v-chip>

                <v-btn
                  v-if="service.credentials?.required"
                  icon="mdi-key"
                  size="small"
                  variant="tonal"
                  class="credentials-btn"
                  :color="getCredentialsColor(service)"
                  :title="getCredentialsTitle(service)"
                  @click="openCredentials(service.id)"
                />
              </div>
            </template>
          </v-list-item>
        </v-list>

        <v-dialog
          :model-value="enableConfirmServiceId !== null"
          max-width="480"
          @update:model-value="!$event && cancelEnable()"
        >
          <v-card>
            <v-card-title>{{ enableConfirmDialogTitle }}</v-card-title>
            <v-card-text>{{ t('services.enableWarning.message') }}</v-card-text>
            <v-card-actions class="enable-warning-actions">
              <v-btn
                color="primary"
                variant="elevated"
                @click="enableWithCredentials"
              >
                {{ t('services.enableWarning.addCredentials') }}
              </v-btn>
              <v-btn
                color="primary"
                variant="tonal"
                :loading="isLoading"
                @click="enableWithoutCredentials"
              >
                {{ t('services.enableWarning.activateAnyway') }}
              </v-btn>
              <v-btn
                color="primary"
                variant="text"
                :disabled="isLoading"
                @click="cancelEnable"
              >
                {{ t('common.cancel') }}
              </v-btn>
            </v-card-actions>
          </v-card>
        </v-dialog>

        <CredentialsDialog
          :service-id="activeDialog"
          :service-title="activeDialogTitle"
          @close="onCredentialsClosed"
          @saved="onCredentialsSaved"
        />
      </v-col>
    </v-row>
  </v-container>
</template>




<style scoped>
/* Logo and fallback logo - shared styles */
.service-logo {
  width: 120px;
  height: 120px;
  background-color: rgba(128, 128, 128, 0.5);
  border-radius: 4px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 4px;
  margin: 0 10px;
}

.service-logo .v-img {
  max-width: 100%;
  max-height: 100%;
}

.service-fallback {
  font-size: 3rem;
  font-weight: bold;
}

/* Align list item content to top and prevent text truncation */
:deep(.v-list-item__content) {
  align-self: flex-start !important;
  margin-right: 10px !important;
  overflow: visible !important;
  text-overflow: unset !important;
  white-space: normal !important;
}

/* Description on multiple lines: override Vuetify 3 subtitle clamping */
:deep(.v-list-item-subtitle) {
  overflow: visible !important;
  text-overflow: unset !important;
  white-space: normal !important;
  -webkit-line-clamp: unset !important;
  display: block !important;
  -webkit-box-orient: unset !important;
}

/* Remove line clamp from list */
:deep(.v-list-item) {
  -webkit-line-clamp: unset !important;
}

/* Center switch vertically */
:deep(.v-switch) {
  align-self: center !important;
}

/* Stack the enable-confirmation buttons vertically, full width */
.enable-warning-actions {
  flex-direction: column !important;
  align-items: stretch !important;
  gap: 8px !important;
}

.enable-warning-actions .v-btn {
  margin: 0 !important;
}

/* Make the credentials key read as a real button: background tint + border. */
.credentials-btn {
  border: 1px solid currentColor;
}

:deep(.v-input__control) {
  align-self: center !important;
}
</style>