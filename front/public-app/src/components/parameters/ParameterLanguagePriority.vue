<script setup lang="ts">
import { ref, watch } from 'vue'
import { normalizeLanguageCode } from '@/utils'
import { DEFAULT_SUBTITLE_LANGUAGE, formatLanguageLabel } from '@/services/languages'
import { useI18n } from '@/i18n'

/**
 * Props accepted by the ordered subtitle language priority control.
 */
interface Props {
  /**
   * Currently ordered language codes, highest priority first.
   */
  modelValue: string[]
  /**
   * Accessible label for the list.
   */
  label: string
  /**
   * Hint displayed below the label.
   */
  hint: string
  /**
   * Placeholder shown in the add-language input.
   * @default DEFAULT_SUBTITLE_LANGUAGE
   */
  placeholder?: string
}

const { t } = useI18n()

const props = withDefaults(defineProps<Props>(), {
  placeholder: DEFAULT_SUBTITLE_LANGUAGE,
})

const emit = defineEmits<{
  /** Emitted when the ordered language list changes. */
  'update:modelValue': [value: string[]]
}>()

/** Draft language code typed by the user. */
const draftLanguage = ref('')
/** Working copy of the ordered language list. */
const languages = ref([...props.modelValue])

watch(() => props.modelValue, (nextValue) => {
  languages.value = [...nextValue]
}, { deep: true })

/**
 * Normalizes a raw language code using the shared BCP-47 normalizer.
 *
 * @param value Raw language code.
 * @returns Normalized code, or `null` when invalid.
 */
function normalizeInput(value: string): string | null {
  return normalizeLanguageCode(value)
}

/**
 * Adds the drafted language to the ordered list after validation.
 */
function handleAddLanguage(): void {
  const normalized = normalizeInput(draftLanguage.value)
  if (!normalized || languages.value.includes(normalized)) {
    draftLanguage.value = ''
    return
  }

  languages.value = [...languages.value, normalized]
  emit('update:modelValue', languages.value)
  draftLanguage.value = ''
}

/**
 * Removes one language from the ordered list.
 *
 * @param code Language code to remove.
 */
function handleRemoveLanguage(code: string): void {
  languages.value = languages.value.filter((existing) => existing !== code)
  emit('update:modelValue', languages.value)
}

/**
 * Moves one language up or down in the priority order.
 *
 * @param index Index of the language to move.
 * @param direction `-1` to move up, `1` to move down.
 */
function moveLanguage(index: number, direction: -1 | 1): void {
  const targetIndex = index + direction
  if (targetIndex < 0 || targetIndex >= languages.value.length) {
    return
  }

  const next = [...languages.value]
  const current = next[index]
  const target = next[targetIndex]
  if (current === undefined || target === undefined) {
    return
  }
  next[index] = target
  next[targetIndex] = current
  languages.value = next
  emit('update:modelValue', languages.value)
}

/**
 * Returns whether the drafted language can be added.
 *
 * @returns True when the draft is a valid, non-duplicate language code.
 */
function canAddLanguage(): boolean {
  const normalized = normalizeInput(draftLanguage.value)
  return Boolean(normalized && !languages.value.includes(normalized))
}

/**
 * Forwards the drafted language input value.
 *
 * @param event Native input event.
 */
function handleDraftInput(event: Event): void {
  const target = event.target as HTMLInputElement
  draftLanguage.value = target.value
}
</script>

<template>
  <div class="parameter-language-priority" :aria-label="label">
    <p class="parameter-language-priority__label">{{ label }}</p>
    <p class="parameter-language-priority__hint">{{ hint }}</p>

    <ul class="parameter-language-priority__list">
      <li
        v-for="(code, index) in languages"
        :key="code"
        class="parameter-language-priority__item"
      >
        <span class="parameter-language-priority__code" :title="code">{{ formatLanguageLabel(code) }}</span>

        <button
          type="button"
          class="parameter-language-priority__button parameter-language-priority__button--up"
          :disabled="index === 0"
          @click="moveLanguage(index, -1)"
          :aria-label="t('settings.playerTracks.moveLanguageUp', { code })"
        >
          ▲
        </button>

        <button
          type="button"
          class="parameter-language-priority__button parameter-language-priority__button--down"
          :disabled="index === languages.length - 1"
          @click="moveLanguage(index, 1)"
          :aria-label="t('settings.playerTracks.moveLanguageDown', { code })"
        >
          ▼
        </button>

        <button
          type="button"
          class="parameter-language-priority__button parameter-language-priority__button--remove"
          @click="handleRemoveLanguage(code)"
          :aria-label="t('settings.playerTracks.removeLanguage', { code })"
        >
          ✕
        </button>
      </li>
    </ul>

    <div class="parameter-language-priority__input">
      <input
        type="text"
        :placeholder="placeholder"
        :value="draftLanguage"
        @input="handleDraftInput"
        @keydown.enter.prevent="handleAddLanguage"
        :aria-label="t('settings.playerTracks.addLanguage')"
        maxlength="8"
      />

      <button
        type="button"
        class="parameter-language-priority__button parameter-language-priority__button--add"
        :disabled="!canAddLanguage()"
        @click="handleAddLanguage"
      >
        +
      </button>
    </div>
  </div>
</template>

<style scoped>
.parameter-language-priority__label {
  margin: 0 0 4px 0;
  color: var(--text-primary);
  font-size: 0.92rem;
  font-weight: 700;
}

.parameter-language-priority__hint {
  margin: 0 0 8px 0;
  color: var(--text-secondary);
  font-size: 0.85rem;
}

.parameter-language-priority__list {
  list-style: none;
  padding: 0;
  margin: 0 0 8px 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.parameter-language-priority__item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
}

.parameter-language-priority__code {
  flex: 1;
  font-size: 0.9rem;
  color: var(--text-primary);
}

.parameter-language-priority__button {
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

.parameter-language-priority__button:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.parameter-language-priority__button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.parameter-language-priority__button--remove {
  color: var(--text-danger);
  border-color: var(--border-color-danger);
}

.parameter-language-priority__button--add {
  color: var(--text-primary);
  border-color: var(--border-color-primary);
}

.parameter-language-priority__input {
  display: flex;
  gap: 6px;
}

.parameter-language-priority__input input {
  flex: 1;
  min-height: 34px;
  padding: 6px 10px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
}

.parameter-language-priority__input input:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>
