<script setup lang="ts">
import { ref, watch } from 'vue'
import { LANGUAGES, formatLanguageLabel } from '@/services/languages'
import { useI18n } from '@/i18n'

/**
 * Props accepted by the subtitle language list control.
 */
interface Props {
  /**
   * Currently configured subtitle language codes, highest priority first.
   */
  modelValue: string[]
  /**
   * Accessible label for the list.
   */
  label: string
}

const props = withDefaults(defineProps<Props>(), {})

const { t } = useI18n()

const emit = defineEmits<{
  /** Emitted when the ordered language list changes. */
  'update:modelValue': [value: string[]]
}>()

/** Working copy of the configured language list. */
const languages = ref([...props.modelValue])

watch(() => props.modelValue, (nextValue) => {
  languages.value = [...nextValue]
}, { deep: true })

/** Available subtitle language options derived from the shared catalog. */
const languageOptions = LANGUAGES.map((language) => ({
  value: language.code,
  label: formatLanguageLabel(language.code),
}))

/**
 * Updates the language code at one row.
 *
 * @param index Row index to update.
 * @param value Newly selected language code.
 */
function handleLanguageUpdate(index: number, value: string): void {
  const next = [...languages.value]
  next[index] = value
  languages.value = next
  emit('update:modelValue', languages.value)
}

/**
 * Removes the language at the given row. The first row cannot be removed.
 *
 * @param index Row index to remove.
 */
function handleRemoveLanguage(index: number): void {
  if (languages.value.length <= 1 || index === 0) {
    return
  }

  const next = [...languages.value]
  next.splice(index, 1)
  languages.value = next
  emit('update:modelValue', languages.value)
}

/**
 * Adds a new language row at the end of the list.
 */
function handleAddLanguage(): void {
  const next = [...languages.value, '']
  languages.value = next
  emit('update:modelValue', languages.value)
}
</script>

<template>
  <div class="parameter-subtitle-languages">
    <span class="parameter-subtitle-languages__label">{{ label }}</span>

    <div class="parameter-subtitle-languages__lines">
      <div
        v-for="(code, index) in languages"
        :key="index"
        class="parameter-subtitle-languages__line"
      >
        <select
          class="parameter-subtitle-languages__select"
          :value="code"
          @change="(event: Event) => handleLanguageUpdate(index, (event.target as HTMLSelectElement).value)"
        >
          <option value="">{{ t('settings.subtitleLanguages.chooseLanguage') }}</option>
          <option
            v-for="language in languageOptions"
            :key="language.value"
            :value="language.value"
          >
            {{ language.label }}
          </option>
        </select>

        <button
          type="button"
          class="parameter-subtitle-languages__button parameter-subtitle-languages__button--remove"
          :class="{ 'parameter-subtitle-languages__button--hidden': index === 0 }"
          @click="handleRemoveLanguage(index)"
          :aria-label="t('settings.subtitleLanguages.removeLanguage', { code })"
        >
          −
        </button>
        <button
          type="button"
          class="parameter-subtitle-languages__button parameter-subtitle-languages__button--add"
          :class="{ 'parameter-subtitle-languages__button--hidden': index !== languages.length - 1 }"
          @click="handleAddLanguage"
          :aria-label="t('settings.subtitleLanguages.addLanguage')"
        >
          +
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.parameter-subtitle-languages {
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 100%;
  flex: 1 1 100%;
}

.parameter-subtitle-languages__label {
  display: block;
  color: var(--text-primary);
  font-size: 0.92rem;
  font-weight: 700;
  margin-bottom: 8px;
  white-space: nowrap;
  margin-left: 10px;
  line-height: 1.2;
}

.parameter-subtitle-languages__lines {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.parameter-subtitle-languages__line {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-left: 20px;
}

.parameter-subtitle-languages__select {
  flex: 1;
  min-width: 0;
  min-height: 40px;
}

.parameter-subtitle-languages__button {
  width: 28px;
  height: 28px;
  border: 1px solid var(--border-color-primary);
  border-radius: 6px;
  background: var(--bg-surface);
  color: var(--text-primary);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 0.9rem;
  line-height: 1;
}

.parameter-subtitle-languages__button:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.parameter-subtitle-languages__button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.parameter-subtitle-languages__button--remove {
  color: var(--text-danger);
  border-color: var(--border-color-danger);
}

.parameter-subtitle-languages__button--add {
  color: var(--text-primary);
  border-color: var(--border-color-primary);
}

.parameter-subtitle-languages__button--hidden {
  visibility: hidden;
}

@media (max-width: 720px) {
  .parameter-subtitle-languages__line {
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
  }

  .parameter-subtitle-languages__button {
    width: auto;
    padding: 0 10px;
  }
}
</style>
