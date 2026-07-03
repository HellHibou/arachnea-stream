<script setup lang="ts">
import { ref } from 'vue'
import type { SearchFilterOption } from '@/services/rustify'
import { useI18n } from '@/i18n'

/**
 * Props accepted by the search form component.
 */
interface Props {
  /**
   * Controlled search query value.
   */
  modelValue: string
  /**
   * Placeholder shown when the field is empty.
   * @default 'Chercher...'
   */
  placeholder?: string
  /**
   * Label displayed on the submit button.
   * @default 'Chercher'
   */
  buttonLabel?: string
  /**
   * Available media type filter values.
   * @default []
   */
  mediaTypeOptions?: SearchFilterOption[]
  /**
   * Selected media type values.
   * @default []
   */
  selectedMediaTypes?: string[]
  /**
   * Available theme filter values.
   * @default []
   */
  themeOptions?: SearchFilterOption[]
  /**
   * Selected theme values.
   * @default []
   */
  selectedThemes?: string[]
}

/** Component props with applied defaults. */
withDefaults(defineProps<Props>(), {
  placeholder: 'Chercher...',
  buttonLabel: 'Chercher',
  mediaTypeOptions: () => [],
  selectedMediaTypes: () => [],
  themeOptions: () => [],
  selectedThemes: () => [],
})

const emit = defineEmits<{
  /** Emitted when the search query changes. */
  'update:modelValue': [value: string]
  /** Emitted when selected media types change. */
  'update:selectedMediaTypes': [value: string[]]
  /** Emitted when selected themes change. */
  'update:selectedThemes': [value: string[]]
  /** Emitted when the search form is submitted. */
  submit: []
}>()

/** Internationalization utilities. */
const { t } = useI18n()

/**
 * Reference to the search input element for programmatic focus.
 */
const searchInputRef = ref<HTMLInputElement | null>(null)

/**
 * Emits the updated field value so the parent keeps the search state in sync.
 * @param event Input event fired by the search field.
 */
function onInput(event: Event) {
  const target = event.target as HTMLInputElement
  emit('update:modelValue', target.value)
}

/**
 * Emits the search submit event without reloading the page.
 */
function onSubmit() {
  emit('submit')
}

/**
 * Updates one single-select filter from the selected browser option.
 * @param event Change event fired by the select element.
 * @param targetModel Model update event to emit.
 */
function onSelectChange(
  event: Event,
  targetModel: 'update:selectedMediaTypes' | 'update:selectedThemes',
) {
  const target = event.target as HTMLSelectElement
  const value = target.value
  const values = value ? [value] : []

  if (targetModel === 'update:selectedMediaTypes') {
    emit('update:selectedMediaTypes', values)
    return
  }

  emit('update:selectedThemes', values)
}

/**
 * Focuses the search input element.
 */
function focus() {
  searchInputRef.value?.focus()
}

defineExpose({ focus })
</script>

<template>
  <form class="search-bar" role="search" @submit.prevent="onSubmit">
    <label class="search-bar__field">
      <!-- Keep a real label for screen readers while leaving the visual layout uncluttered. -->
      <span class="search-bar__label">{{ t('search.label') }}</span>
      <v-icon class="search-bar__field-icon" icon="$Search" size="22" aria-hidden="true" />
      <input
        ref="searchInputRef"
        class="search-bar__input"
        :placeholder="placeholder"
        :value="modelValue"
        type="search"
        @input="onInput"
      />
    </label>

    <div class="search-bar__filters">
      <label class="search-bar__select-field">
        <span class="search-bar__label">{{ t('filters.type') }}</span>
        <select
          class="search-bar__select"
          :value="selectedMediaTypes[0] ?? ''"
          @change="onSelectChange($event, 'update:selectedMediaTypes')"
        >
          <option v-for="option in mediaTypeOptions" :key="option.value" :value="option.value">
            {{ option.label }}
          </option>
        </select>
      </label>

      <label class="search-bar__select-field">
        <span class="search-bar__label">{{ t('filters.theme') }}</span>
        <select
          class="search-bar__select"
          :value="selectedThemes[0] ?? ''"
          @change="onSelectChange($event, 'update:selectedThemes')"
        >
          <option v-for="option in themeOptions" :key="option.value" :value="option.value">
            {{ option.label }}
          </option>
        </select>
      </label>
    </div>

    <button class="search-bar__button" type="submit">
      {{ buttonLabel }}
    </button>
  </form>
</template>

<style scoped>
.search-bar {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto;
  gap: 10px;
  align-items: center;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-transparent);
  box-shadow:
    var(--shadow-heavy),
    var(--inset-light);
  backdrop-filter: var(--backdrop-filter-medium);
}

.search-bar__field {
  position: relative;
  display: block;
}

.search-bar__select-field {
  display: block;
}

.search-bar__filters {
  display: flex;
  align-items: center;
  gap: 10px;
}

.search-bar__field-icon {
  position: absolute;
  top: 50%;
  left: 18px;
  transform: translateY(-50%);
  color: var(--text-disabled);
  pointer-events: none;
}

.search-bar__label {
  position: absolute;
  width: 1px;
  height: 1px;
  margin: -1px;
  padding: 0;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}

.search-bar__input {
  width: 100%;
  min-height: 40px;
  padding: 0 18px 0 52px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  outline: none;
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
  font-size: 1.05rem;
  letter-spacing: 0.01em;
  transition:
    border-color var(--duration-fast) ease,
    box-shadow var(--duration-fast) ease,
    background-color var(--duration-fast) ease;
}

.search-bar__select {
  min-width: 170px;
  min-height: 40px;
  padding: 10px 14px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
}

.search-bar__select:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.search-bar__input::placeholder {
  color: var(--text-disabled);
}

.search-bar__input:focus {
  border-color: var(--color-primary);
  background: var(--bg-surface);
}

.search-bar__input:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.search-bar__button {
  min-width: 116px;
  min-height: 40px;
  padding: 0 20px;
  border: 0;
  border-radius: var(--radius);
  background: var(--bg-accent-blue);
  color: var(--text-primary);
  font: inherit;
  font-size: 1rem;
  font-weight: 700;
  letter-spacing: 0.01em;
  cursor: pointer;
  transition:
    transform var(--duration-fast) ease,
    box-shadow var(--duration-fast) ease,
    filter var(--duration-fast) ease;
}

.search-bar__button:hover {
  transform: translateY(-1px);
  filter: brightness(1.04);
}
.search-bar__button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}


@media (max-width: 720px) {
  .search-bar {
    grid-template-columns: 1fr;
    padding: 10px;
  }

  .search-bar__filters {
    display: grid;
    grid-template-columns: 1fr;
  }

  .search-bar__select {
    width: 100%;
    min-width: 0;
  }

  .search-bar__button {
    width: 100%;
  }
}
</style>
