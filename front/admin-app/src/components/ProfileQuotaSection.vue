<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { createOperationId } from '@/services/operationId'
import { useI18n } from '@/i18n'
import { fetchProfileQuota, saveProfileQuota } from '@/services/adminApi'

const { t } = useI18n()
const draft = ref<number | string>('')
const loaded = ref(false)
const busy = ref(false)
const error = ref('')
const success = ref(false)
let pending: { id: string; value: number } | null = null

async function load(): Promise<void> {
  busy.value = true
  error.value = ''
  try {
    draft.value = await fetchProfileQuota()
    loaded.value = true
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    busy.value = false
  }
}

async function save(): Promise<void> {
  error.value = ''
  success.value = false
  const value = Number(draft.value)
  if (!Number.isSafeInteger(value) || value < 1) {
    error.value = t('profileQuota.invalid')
    return
  }
  busy.value = true
  pending = pending?.value === value ? pending : { id: createOperationId(), value }
  try {
    draft.value = await saveProfileQuota(pending.id, value)
    pending = null
    success.value = true
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    busy.value = false
  }
}

onMounted(load)
</script>

<template>
  <v-card variant="outlined" class="mb-6">
    <v-card-title>{{ t('profileQuota.title') }}</v-card-title>
    <v-card-text>
      <v-alert v-if="error" type="error" variant="tonal" class="mb-4" :text="error" />
      <v-alert v-if="success" type="success" variant="tonal" class="mb-4" :text="t('profileQuota.saved')" />
      <v-form v-if="loaded" @submit.prevent="save">
        <v-text-field v-model="draft" type="number" min="1" step="1" :max="Number.MAX_SAFE_INTEGER"
          :label="t('profileQuota.limit')" :hint="t('profileQuota.hint')" persistent-hint
          variant="outlined" :disabled="busy" @update:model-value="success = false" />
        <v-btn type="submit" color="primary" :loading="busy">{{ t('profileQuota.save') }}</v-btn>
      </v-form>
      <v-btn v-else :loading="busy" @click="load">{{ t('profileQuota.retry') }}</v-btn>
    </v-card-text>
  </v-card>
</template>
