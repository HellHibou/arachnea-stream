<script setup lang="ts">
import { computed, useId } from 'vue'
import { useI18n } from '@/i18n'
import { themePresets } from '@/services/theme'
import ParameterThemeSelector from '@/components/parameters/ParameterThemeSelector.vue'

/** Common profile preferences edited by the guide before creation. */
const props = withDefaults(defineProps<{
  language: string | null
  theme: string
  busy?: boolean
}>(), { busy: false })

const emit = defineEmits<{
  'update:language': [value: string | null]
  'update:theme': [value: string]
}>()

const { languageOptions, t } = useI18n()
const componentId = useId()
const languageInputId = `${componentId}-language`
const themeInputName = `${componentId}-theme`

/** Theme swatches derived from the loaded presets. */
const themeOptions = computed(() => themePresets.map((preset) => ({
  value: preset.key,
  label: preset.label,
  color: `hsl(${preset.h}, ${preset.s}, ${preset.l})`,
})))

/** Maps the optional language preference to the explicit Auto option. */
const selectedLanguage = computed(() => props.language ?? 'auto')

/** @param event Change event from the language select. */
function updateLanguage(event: Event): void {
  const value = (event.target as HTMLSelectElement).value
  emit('update:language', value === 'auto' ? null : value)
}

/** @param value Selected theme preset key. */
function updateTheme(value: string): void {
  emit('update:theme', value)
}
</script>

<template>
  <div class="profile-preferences">
    <label class="profile-preferences__field" :for="languageInputId">
      <span>{{ t('settings.language.label') }}</span>
      <select :id="languageInputId" class="profile-preferences__select" :value="selectedLanguage" :disabled="busy" @change="updateLanguage">
        <option v-for="language in languageOptions" :key="language.code" :value="language.code">{{ language.label }}</option>
      </select>
    </label>

    <div v-if="themeOptions.length > 1" class="profile-preferences__field">
      <span>{{ t('settings.colorTheme') }}</span>
      <ParameterThemeSelector :input-name="themeInputName" :model-value="theme" :options="themeOptions" @update:model-value="updateTheme" />
    </div>
  </div>
</template>

<style scoped>
.profile-preferences {
  display: grid;
  gap: 16px;
  width: min(520px, 100%);
}

.profile-preferences__field {
  display: grid;
  gap: 6px;
  font-weight: 600;
}

.profile-preferences__select {
  min-height: 40px;
  padding: 8px 14px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
}

.profile-preferences__select:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>
