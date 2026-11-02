<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from '@/i18n'
import type { ProxySettingsResponse, ProxySettingsUpdate, SettingSource } from '@/services/adminApi'
import ProxyApiKeysSection from './ProxyApiKeysSection.vue'

const props = defineProps<{ settings: ProxySettingsResponse; modelValue: ProxySettingsUpdate; loading: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [value: ProxySettingsUpdate] }>()
const { t } = useI18n()
const modes = computed(() => ['local', 'private', 'public'].map(value => ({ value, title: t(`settings.networkMode${value[0]!.toUpperCase()}${value.slice(1)}`) })))
const pinnedSettings = computed(() => [
  props.settings.enabled_source === 'command_line' ? `${t('proxy.enabled')}: ${props.settings.enabled}` : '',
  props.settings.server_port_source === 'command_line' ? `${t('settings.serverPort')}: ${props.settings.server_port ?? 0}` : '',
  props.settings.network_mode_source === 'command_line' ? `${t('settings.networkMode')}: ${props.settings.network_mode}` : '',
].filter(Boolean).join(' · '))
function update(patch: ProxySettingsUpdate): void {
  emit('update:modelValue', { ...props.modelValue, ...patch })
}
function hint(source: SettingSource): string | undefined {
  return source === 'default' ? undefined : t('settings.source', { source: t(source === 'command_line' ? 'settings.sourceCommandLine' : 'settings.sourceConfiguration') })
}
</script>

<template>
  <v-card variant="outlined" class="mb-6">
    <v-card-title>{{ t('proxy.title') }}</v-card-title>
    <v-card-text>
      <v-alert v-if="pinnedSettings" type="info" variant="tonal" class="mb-4" :text="t('proxy.pinned', { settings: pinnedSettings })" />
      <v-switch v-if="settings.enabled_source !== 'command_line'" :model-value="modelValue.enabled" :label="t('proxy.enabled')" :disabled="loading" color="primary" :hint="hint(settings.enabled_source)" persistent-hint @update:model-value="update({ enabled: !!$event })" />
      <v-alert v-if="!modelValue.enabled" type="info" variant="tonal" class="mb-4" :text="t('proxy.disabledHint')" />
      <p class="text-body-2 mb-4">{{ t('proxy.effective', { port: settings.effective_port, network: settings.effective_network_mode }) }}</p>
      <v-alert v-if="settings.apply_error" type="error" variant="tonal" class="mb-4" :text="settings.apply_error" />
      <v-text-field v-if="settings.server_port_source !== 'command_line'" :model-value="modelValue.server_port" :label="t('settings.serverPort')" type="number" :min="1" :max="65535" :disabled="!modelValue.enabled || loading" variant="outlined" :hint="hint(settings.server_port_source)" persistent-hint @update:model-value="update({ server_port: $event === '' || $event === null ? undefined : Number($event) })" />
      <v-select v-if="settings.network_mode_source !== 'command_line'" :model-value="modelValue.network_mode" :label="t('settings.networkMode')" :items="modes" :disabled="!modelValue.enabled || loading" variant="outlined" class="mt-4" :hint="hint(settings.network_mode_source)" persistent-hint @update:model-value="update({ network_mode: $event })" />
      <v-alert v-if="modelValue.enabled && modelValue.network_mode !== 'local'" type="warning" variant="tonal" class="mt-2" :text="t('proxy.networkWarning')" />
      <ProxyApiKeysSection :keys="settings.keys" :disabled="loading" />
    </v-card-text>
  </v-card>
</template>
