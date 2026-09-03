<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { useAdminApi } from '@/composables/useAdminApi'
import { useI18n } from '@/i18n'
import { hasConfiguredServiceStore, defaultServicesPath } from '@/services/appConfig'
import type {
  AdminServiceEntry,
  AdminUnavailableSource,
  ServicesResponse,
} from '@/services/adminApi'
import CredentialsDialog from '@/components/CredentialsDialog.vue'

const route = useRoute()
const router = useRouter()

const { t, serviceStoreTitle, serviceStoreDescription } = useI18n()
const {
  getServices,
  setServiceEnabled,
  resetServiceEnabled,
  reload,
  isLoading,
  error,
} = useAdminApi()

/** The service store (group) displayed, from the `/services/:serviceStoreId` route parameter. */
const serviceStoreId = computed(() => {
  const raw = route.params.serviceStoreId
  return Array.isArray(raw) ? (raw[0] ?? '') : (raw ?? '')
})

/** Composite identity of a source within the administration catalog. */
function serviceKey(service: AdminServiceEntry): string {
  return `${service.service_store_id}/${service.id}`
}

const groupTitle = computed(() => serviceStoreTitle(serviceStoreId.value))
const groupDescription = computed(() => serviceStoreDescription(serviceStoreId.value))

const services = ref<AdminServiceEntry[]>([])
const unavailable = ref<AdminUnavailableSource[]>([])
const needsReload = ref(false)
const reloadResult = ref<string | null>(null)
const reloadDetails = ref<ReloadSummary | null>(null)
/** Composite key of the service currently opened in the credentials dialog. */
const activeDialogKey = ref<string | null>(null)
/** Composite key of a service awaiting activation after credentials are saved (null otherwise). */
const pendingActivationKey = ref<string | null>(null)
/** Composite key of the service for which the "credentials required" confirmation dialog is open. */
const enableConfirmKey = ref<string | null>(null)
/** Bumped to force switch re-creation so cancelled toggles revert visually. */
const switchEpoch = ref(0)

/**
 * Summary of a configuration reload result for display in the admin UI.
 */
interface ReloadSummary {
  applied: boolean
  loaded: number
  disabled: number
  ignored: number
  errors: number
  errorDetails: string[]
  buildError?: string
}

const servicesWithCredentials = computed(() =>
  services.value.filter((s) => s.credentials?.required),
)

/** Catalog entry currently opened in the credentials dialog. */
const activeService = computed(() => {
  if (!activeDialogKey.value) {
    return null
  }
  return services.value.find((s) => serviceKey(s) === activeDialogKey.value) ?? null
})

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
  const service = enableConfirmKey.value
    ? services.value.find((s) => serviceKey(s) === enableConfirmKey.value)
    : undefined
  const label = service
    ? (service.title ? `${service.title} (${service.id})` : service.id)
    : ''
  return t('services.enableWarning.title', { service: label })
})

async function loadServices(): Promise<void> {
  const result = await getServices()
  if (result) {
    updateGroupData(result)
  }
}

/**
 * Updates the displayed group data from the `services` catalog response.
 *
 * Prefers the grouped `service_stores` contract; falls back to grouping the
 * flat `services` list when an older backend omits the grouped sections.
 * Groups reported by the backend but absent from `config.json` are hidden and
 * only signalled in the console, exposing a frontend/backend divergence.
 */
function updateGroupData(result: ServicesResponse): void {
  reportUnexplainedGroups(result)

  const store = (result.service_stores ?? []).find(
    (candidate) => candidate.service_store_id === serviceStoreId.value,
  )
  if (store) {
    services.value = store.services
    unavailable.value = store.unavailable ?? []
    return
  }

  services.value = result.services.filter(
    (service) => service.service_store_id === serviceStoreId.value,
  )
  unavailable.value = result.unavailable ?? []
}

function reportUnexplainedGroups(result: ServicesResponse): void {
  for (const store of result.service_stores ?? []) {
    if (!hasConfiguredServiceStore(store.service_store_id)) {
      console.warn(
        `[admin] The backend reports service group "${store.service_store_id}" which is not declared in config.json; it is hidden from the UI.`,
      )
    }
  }
}

/** Redirects unknown `serviceStoreId` route values to the first configured group. */
function ensureKnownGroup(): void {
  const id = serviceStoreId.value
  if (id && !hasConfiguredServiceStore(id)) {
    router.replace(defaultServicesPath())
  }
}

async function handleSetEnabled(service: AdminServiceEntry, enabled: boolean): Promise<void> {
  const result = await setServiceEnabled(service.service_store_id, service.id, enabled)
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
function requestEnable(service: AdminServiceEntry): void {
  if (service.credentials?.required && !service.credentials?.configured) {
    enableConfirmKey.value = serviceKey(service)
    return
  }
  void handleSetEnabled(service, true)
}

/** Leaves the service disabled (user dismissed the confirmation dialog). */
async function cancelEnable(): Promise<void> {
  enableConfirmKey.value = null
  switchEpoch.value += 1
  await loadServices()
}

/** Opens the credentials dialog; activation resumes after a successful save. */
function enableWithCredentials(): void {
  const key = enableConfirmKey.value
  enableConfirmKey.value = null
  if (!key) {
    return
  }
  pendingActivationKey.value = key
  activeDialogKey.value = key
}

/** Activates the service without configuring credentials. */
function enableWithoutCredentials(): void {
  const key = enableConfirmKey.value
  enableConfirmKey.value = null
  if (!key) {
    return
  }
  const service = services.value.find((candidate) => serviceKey(candidate) === key)
  if (service) {
    void handleSetEnabled(service, true)
  }
}

/** Activates the pending service after credentials were saved. */
async function onCredentialsSaved(): Promise<void> {
  const key = pendingActivationKey.value
  pendingActivationKey.value = null
  if (key) {
    const service = services.value.find((candidate) => serviceKey(candidate) === key)
    if (service) {
      await handleSetEnabled(service, true)
    }
    return
  }
  await loadServices()
}

/** Drops the pending activation when the credentials dialog is cancelled. */
async function onCredentialsClosed(): Promise<void> {
  if (pendingActivationKey.value) {
    pendingActivationKey.value = null
    switchEpoch.value += 1
    await loadServices()
  }
  closeCredentials()
}

async function handleResetEnabled(service: AdminServiceEntry): Promise<void> {
  const result = await resetServiceEnabled(service.service_store_id, service.id)
  if (result?.reload_required) {
    needsReload.value = true
    reloadResult.value = t('services.reloadRequired')
  }
  await loadServices()
}

async function handleReload(): Promise<void> {
  const result = await reload()
  reloadDetails.value = null
  if (result?.applied) {
    needsReload.value = false
    reloadResult.value = t('reload.success')
    reloadDetails.value = {
      applied: true,
      loaded: result.loaded?.length ?? 0,
      disabled: result.disabled?.length ?? 0,
      ignored: result.ignored?.length ?? 0,
      errors: result.errors?.length ?? 0,
      errorDetails: result.errors?.map((e) => `${e.path}: ${e.message}`) ?? [],
    }
  } else {
    reloadResult.value = result?.build_error ?? t('reload.failed')
    reloadDetails.value = {
      applied: false,
      loaded: result?.loaded?.length ?? 0,
      disabled: result?.disabled?.length ?? 0,
      ignored: result?.ignored?.length ?? 0,
      errors: result?.errors?.length ?? 0,
      errorDetails: result?.errors?.map((e) => `${e.path}: ${e.message}`) ?? [],
      buildError: result?.build_error,
    }
  }
  await loadServices()
}

function openCredentials(service: AdminServiceEntry): void {
  activeDialogKey.value = serviceKey(service)
}

function closeCredentials(): void {
  activeDialogKey.value = null
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
  ensureKnownGroup()
  loadServices()
})

// Reload the group data when navigation switches between service groups.
watch(serviceStoreId, () => {
  ensureKnownGroup()
  loadServices()
})
</script>

<template>
  <v-container fluid>
    <v-row>
      <v-col cols="12">
        <h1 class="text-h4 mb-2">{{ groupTitle }}</h1>
        <p
          v-if="groupDescription"
          class="text-body-1 text-medium-emphasis mb-4"
        >
          {{ groupDescription }}
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
          :type="reloadDetails?.applied ? 'success' : 'error'"
          variant="tonal"
          class="mb-4"
        >
          <div class="d-flex align-center justify-space-between mb-2">
            <span>{{ reloadResult }}</span>
          </div>
          <div v-if="reloadDetails" class="text-body-2">
            <div>
              {{ t('reload.details', {
                loaded: reloadDetails.loaded,
                disabled: reloadDetails.disabled,
                ignored: reloadDetails.ignored,
                errors: reloadDetails.errors,
              }) }}
            </div>
            <ul v-if="reloadDetails.errorDetails.length > 0" class="mt-2">
              <li v-for="(error, index) in reloadDetails.errorDetails" :key="index">
                {{ error }}
              </li>
            </ul>
          </div>
        </v-alert>

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
            :key="serviceKey(service)"
            :title="getServiceName(service)"
            :subtitle="service.description"
          >
            <template #prepend>
              <div class="d-flex align-start" style="gap: 10px;">
                <v-switch
                  :key="`${serviceKey(service)}-${switchEpoch}`"
                  :model-value="service.enabled"
                  color="success"
                  hide-details
                  density="compact"
                  :disabled="service.unavailable"
                  @update:model-value="$event === true ? requestEnable(service) : handleSetEnabled(service, false)"
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
                  @click="openCredentials(service)"
                />
              </div>
            </template>
          </v-list-item>
        </v-list>

        <v-dialog
          :model-value="enableConfirmKey !== null"
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
          :service-store-id="activeService?.service_store_id ?? null"
          :service-id="activeService?.id ?? null"
          :service-title="activeDialogTitle"
          @close="onCredentialsClosed"
          @saved="onCredentialsSaved"
        />

        <v-alert
          v-if="unavailable.length > 0"
          type="warning"
          variant="tonal"
          class="mt-4"
        >
          <div class="font-weight-medium mb-1">{{ t('services.unavailableTitle') }}</div>
          <ul class="mb-0">
            <li v-for="(source, index) in unavailable" :key="index">
              {{ source.path }}{{ source.message ? ` — ${source.message}` : '' }}
            </li>
          </ul>
        </v-alert>
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