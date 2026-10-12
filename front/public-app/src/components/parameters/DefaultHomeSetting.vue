<script setup lang="ts">
import { onMounted, useId } from 'vue'
import { useCustomHomeManager } from '@/composables/home/useCustomHomeManager'
import { useStorage } from '@/services/storage'
import { useI18n } from '@/i18n'
import ParametersPanel from './ParametersPanel.vue'

const manager = useCustomHomeManager()
const parameters = useStorage().getParameters()
const { t } = useI18n()
const inputId = useId()
onMounted(() => { void manager.refresh() })
/** @param event Native selection; empty means aggregated home. */
function update(event: Event): void {
  parameters.defaultHomeId.value = (event.target as HTMLSelectElement).value || null
}
</script>

<template>
  <ParametersPanel :title="t('settings.defaultHome')">
    <select :id="inputId" class="default-home-select" :aria-label="t('settings.defaultHome')"
      :value="parameters.defaultHomeId.value ?? ''" @change="update">
      <option value="">{{ t('settings.aggregatedHome') }}</option>
      <option v-for="home in manager.homes.value" :key="home.id" :value="home.id">{{ home.name }}</option>
      <option v-if="parameters.defaultHomeId.value && !manager.homes.value.some((home) => home.id === parameters.defaultHomeId.value)"
        :value="parameters.defaultHomeId.value" disabled>{{ t('settings.missingDefaultHome') }}</option>
    </select>
  </ParametersPanel>
  <p v-if="manager.errorMessage.value" role="alert">{{ manager.errorMessage.value }}</p>
</template>

<style scoped>
.default-home-select {
  width: min(100%, 272px);
  padding: 8px 14px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
}

.default-home-select:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>