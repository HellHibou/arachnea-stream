<script setup lang="ts">
import { onMounted } from 'vue'

import ServiceGroupCard from '@/components/dashboard/ServiceGroupCard.vue'
import { useDashboard } from '@/composables/useDashboard'
import { useI18n } from '@/i18n'

const { t } = useI18n()
const { groups, passwordConfigured, hasUsers, loading, loadFailed, refresh } = useDashboard()

onMounted(refresh)
</script>

<template>
  <v-container fluid>
    <div class="d-flex align-center justify-space-between ga-4 mb-6">
      <h1 class="text-h4">{{ t('dashboard.title') }}</h1>
      <v-btn prepend-icon="mdi-refresh" :loading="loading" @click="refresh">
        {{ t('dashboard.refresh') }}
      </v-btn>
    </div>
    <v-progress-linear v-if="loading" indeterminate color="primary" :aria-label="t('dashboard.title')" />
    <template v-else>
      <v-alert v-if="loadFailed" type="error" variant="tonal" class="mb-4">
        {{ t('dashboard.loadFailed') }}
      </v-alert>
      <v-alert v-if="passwordConfigured === false" type="warning" variant="tonal" class="mb-4">
        {{ t('dashboard.adminPasswordMissing') }}
        <v-btn :to="{ name: 'settings' }" variant="text" size="small" class="ml-2">{{ t('nav.settings') }}</v-btn>
      </v-alert>
      <v-alert v-if="hasUsers === false" type="warning" variant="tonal" class="mb-4">
        {{ t('dashboard.noUsers') }}
        <v-btn :to="{ name: 'users' }" variant="text" size="small" class="ml-2">{{ t('users.title') }}</v-btn>
      </v-alert>
      <v-row>
        <v-col v-for="group in groups" :key="group.id" cols="12" md="6" xl="4">
          <ServiceGroupCard v-bind="group" />
        </v-col>
      </v-row>
    </template>
  </v-container>
</template>
