<script setup lang="ts">
import { onMounted, useId } from 'vue'
import { useCustomHomeManager } from '@/composables/home/useCustomHomeManager'
import { profileDefaultHomeId, loadDefaultHome, saveDefaultHome } from '@/services/profileDefaultHome'
import { reportPersonalDataFailure } from '@/services/personalDataStatus'
import { useI18n } from '@/i18n'
import ParametersPanel from '@/components/parameters/ParametersPanel.vue'

const manager = useCustomHomeManager()
const { t } = useI18n()
const inputId = useId()
onMounted(() => { void manager.refresh(); void loadDefaultHome().catch(reportPersonalDataFailure) })
/** @param event Native selection; empty means aggregated home. */
function update(event: Event): void {
  void saveDefaultHome((event.target as HTMLSelectElement).value || null).catch(reportPersonalDataFailure)
}
</script>

<template>
  <ParametersPanel :title="t('settings.defaultHome')">
    <select :id="inputId" class="default-home-select" :aria-label="t('settings.defaultHome')"
      :value="profileDefaultHomeId ?? ''" @change="update">
      <option value="">{{ t('settings.aggregatedHome') }}</option>
      <option v-for="home in manager.homes.value" :key="home.id" :value="home.id">{{ home.name }}</option>
      <option v-if="profileDefaultHomeId && !manager.homes.value.some((home) => home.id === profileDefaultHomeId)"
        :value="profileDefaultHomeId" disabled>{{ t('settings.missingDefaultHome') }}</option>
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