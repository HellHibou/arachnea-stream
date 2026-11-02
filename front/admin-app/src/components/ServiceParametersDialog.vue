<script setup lang="ts">
import { watch } from 'vue'
import { useServiceParameters } from '@/composables/useServiceParameters'
import { useI18n } from '@/i18n'
import type { AdminServiceEntry } from '@/services/adminApi'

const props = defineProps<{ service: AdminServiceEntry | null }>()
const emit = defineEmits<{ close: []; saved: [] }>()
const { t } = useI18n()
const { parameters, values, loaded, canSave, load, save, reset, restoreDefaults, validate, isLoading, error } = useServiceParameters()

watch(() => props.service, (service) => {
  if (service) void load(service.service_store_id, service.id)
  else reset()
}, { immediate: true })

async function submit() {
  const service = props.service
  if (service && await save(service.service_store_id, service.id)) emit('saved')
}
</script>

<template>
  <v-dialog :model-value="service !== null" max-width="640" :persistent="isLoading"
    @update:model-value="!$event && emit('close')">
    <v-card>
      <v-card-title>{{ t('serviceParameters.title', { service: service?.title ?? service?.id ?? '' }) }}</v-card-title>
      <v-card-text>
        <v-alert type="info" variant="tonal" class="mb-4">{{ t('serviceParameters.pending') }}</v-alert>
        <v-alert v-if="error" type="error" variant="tonal" class="mb-4">{{ error.message }}</v-alert>
        <v-progress-linear v-if="isLoading" indeterminate class="mb-4" />
        <v-form @submit.prevent="submit">
          <template v-for="parameter in parameters" :key="parameter.name">
            <v-select v-if="parameter.editor.type === 'select'" v-model="values[parameter.name]"
              :label="parameter.label" :items="parameter.editor.options" item-title="label" item-value="value"
              :hint="parameter.description ?? t('serviceParameters.default', { value: parameter.default_value })"
              persistent-hint :disabled="isLoading"
              :rules="[(value: string) => validate(parameter, value)]" class="mb-4" />
            <v-text-field v-else v-model="values[parameter.name]" :label="parameter.label"
              :hint="parameter.description ?? t('serviceParameters.default', { value: parameter.default_value })"
              persistent-hint :disabled="isLoading"
              :rules="[(value: string) => validate(parameter, value)]" class="mb-4" />
          </template>
        </v-form>
      </v-card-text>
      <v-card-actions class="flex-wrap">
        <v-btn :disabled="!loaded || isLoading" @click="restoreDefaults">{{ t('serviceParameters.reset') }}</v-btn>
        <v-spacer />
        <v-btn :disabled="isLoading" @click="emit('close')">{{ t('common.cancel') }}</v-btn>
        <v-btn color="primary" :loading="isLoading" :disabled="!canSave || isLoading" @click="submit">
          {{ t('serviceParameters.save') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>
