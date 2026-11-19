<script setup lang="ts">
import { useI18n } from '@/i18n'

defineProps<{
  id: string
  available: boolean
  enabled: number
  total: number
  severity: 'success' | 'warning' | 'error' | 'info'
  messageKey?: string
}>()

const { t, serviceStoreTitle } = useI18n()
</script>

<template>
  <v-card :to="`/services/${id}`" class="h-100" variant="outlined">
    <v-card-text>
      <div class="d-flex align-center ga-3 mb-4">
        <v-icon icon="mdi-server" :color="available ? severity : undefined" size="28" aria-hidden="true" />
        <h2 class="text-h6">{{ serviceStoreTitle(id) }}</h2>
      </div>
      <template v-if="available">
        <div class="d-flex align-center justify-space-between ga-3 mb-2">
          <span class="text-h4">{{ enabled }} / {{ total }}</span>
          <v-chip :color="severity" variant="tonal">{{ t(`dashboard.status.${severity}`) }}</v-chip>
        </div>
        <div class="text-body-2 text-medium-emphasis">{{ t('dashboard.enabledServices') }}</div>
        <v-alert v-if="messageKey" :type="severity" variant="tonal" class="mt-4">
          {{ t(messageKey) }}
        </v-alert>
      </template>
      <p v-else class="text-body-2">{{ t('dashboard.unavailable') }}</p>
    </v-card-text>
  </v-card>
</template>
