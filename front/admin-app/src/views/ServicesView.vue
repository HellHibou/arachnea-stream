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

const servicesWithCredentials = computed(() =>
  services.value.filter((s) => s.credentials?.required),
)

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
  await loadServices()
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
  return service.title ?? service.id
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

        <v-list v-else lines="two">
          <v-list-item
            v-for="service in services"
            :key="service.id"
            :title="getServiceName(service)"
            :subtitle="service.description"
          >
            <template #prepend>
              <v-avatar v-if="service.logo" color="grey-lighten-4" size="40">
                <v-img :src="service.logo" :alt="service.title ?? service.id" />
              </v-avatar>
              <v-avatar v-else color="grey-lighten-4" size="40">
                <span v-if="service.title" class="text-h6">
                  {{ service.title.charAt(0).toUpperCase() }}
                </span>
                <span v-else class="text-h6">
                  {{ t('services.fallbackLogo') }}
                </span>
              </v-avatar>
            </template>

            <template #append>
              <div class="d-flex align-center ga-2">
                <v-chip
                  v-if="service.unavailable"
                  color="error"
                  size="small"
                  variant="tonal"
                >
                  {{ t('services.unavailable') }}
                </v-chip>
                <v-chip
                  v-else-if="service.has_override"
                  color="info"
                  size="small"
                  variant="tonal"
                >
                  {{ t('services.hasOverride') }}
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
                  variant="text"
                  :title="t('services.credentials')"
                  @click="openCredentials(service.id)"
                />

                <v-menu v-if="!service.unavailable">
                  <template #activator="{ props }">
                    <v-btn
                      v-bind="props"
                      icon="mdi-dots-vertical"
                      size="small"
                      variant="text"
                    />
                  </template>
                  <v-list>
                    <v-list-item
                      :disabled="service.enabled"
                      @click="handleSetEnabled(service.id, true)"
                    >
                      <v-list-item-title>{{ t('services.enable') }}</v-list-item-title>
                    </v-list-item>
                    <v-list-item
                      :disabled="!service.enabled"
                      @click="handleSetEnabled(service.id, false)"
                    >
                      <v-list-item-title>{{ t('services.disable') }}</v-list-item-title>
                    </v-list-item>
                    <v-list-item
                      v-if="service.has_override"
                      @click="handleResetEnabled(service.id)"
                    >
                      <v-list-item-title>{{ t('services.reset') }}</v-list-item-title>
                    </v-list-item>
                  </v-list>
                </v-menu>
              </div>
            </template>
          </v-list-item>
        </v-list>

        <CredentialsDialog
          :service-id="activeDialog"
          @close="closeCredentials"
          @saved="loadServices"
        />
      </v-col>
    </v-row>
  </v-container>
</template>
