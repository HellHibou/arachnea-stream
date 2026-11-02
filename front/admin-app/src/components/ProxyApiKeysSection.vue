<script setup lang="ts">
import { ref, watch } from 'vue'
import type { ProxyKeyInfo } from '@/services/adminApi'
import { useProxyApiKeys } from '@/composables/useProxyApiKeys'
import { useI18n } from '@/i18n'

const props = defineProps<{ keys: ProxyKeyInfo[]; disabled: boolean }>()
const { t } = useI18n()
const label = ref('')
const pendingDelete = ref<ProxyKeyInfo | null>(null)
const { keys, token, loading, error, create, revoke, copy } = useProxyApiKeys()
watch(() => props.keys, value => { keys.value = value }, { immediate: true })

async function createKey(): Promise<void> {
  await create(label.value)
  if (token.value) label.value = ''
}

async function confirmDelete(): Promise<void> {
  if (!pendingDelete.value) return
  await revoke(pendingDelete.value.id)
  pendingDelete.value = null
}
</script>

<template>
  <div class="mt-6">
    <h3 class="text-subtitle-1 mb-2">{{ t('proxy.keys') }}</h3>
    <p class="text-body-2 mb-4">{{ t('proxy.credentialsHint') }}</p>
    <v-alert v-if="error" type="error" variant="tonal" class="mb-3" :text="error" />
    <v-alert v-if="token" type="success" variant="tonal" class="mb-4">
      <p class="mb-2">{{ t('proxy.secretOnce') }}</p>
      <v-textarea :model-value="token" :label="t('proxy.token')" readonly rows="2" auto-grow />
      <v-btn variant="text" @click="copy">{{ t('proxy.copy') }}</v-btn>
      <v-btn variant="text" @click="token = ''">{{ t('common.close') }}</v-btn>
    </v-alert>
    <v-form class="d-flex align-center ga-3" @submit.prevent="createKey">
      <v-text-field v-model="label" :label="t('proxy.keyLabel')" :disabled="disabled || loading" maxlength="128" variant="outlined" hide-details="auto" style="min-width: 0" />
      <v-btn class="flex-shrink-0" type="submit" :disabled="disabled || loading || !label.trim() || !!token" :loading="loading" variant="outlined">{{ t('proxy.createKey') }}</v-btn>
    </v-form>
    <v-list v-if="keys.length" class="mt-3">
      <v-list-item v-for="key in keys" :key="key.id" :title="key.label" :subtitle="new Date(key.created_at * 1000).toLocaleString()">
        <template #append>
          <v-btn :disabled="disabled || loading" variant="text" color="error" @click="pendingDelete = key">{{ t('proxy.revoke') }}</v-btn>
        </template>
      </v-list-item>
    </v-list>
    <p v-else class="text-body-2 mt-3">{{ t('proxy.noKeys') }}</p>
    <v-dialog :model-value="!!pendingDelete" max-width="440" @update:model-value="pendingDelete = null">
      <v-card :title="t('proxy.revoke')" :text="t('proxy.revokeConfirm', { label: pendingDelete?.label ?? '' })">
        <v-card-actions>
          <v-btn :disabled="loading" @click="pendingDelete = null">{{ t('common.cancel') }}</v-btn>
          <v-btn color="error" :loading="loading" @click="confirmDelete">{{ t('proxy.revoke') }}</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </div>
</template>
