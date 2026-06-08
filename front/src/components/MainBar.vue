<script setup lang="ts">
import { useId } from 'vue'

import type { SearchFilterOption } from '@/services/rustify'
import MainBarParametersPopover from './main-bar/MainBarParametersPopover.vue'
import MainBarToolbar from './main-bar/MainBarToolbar.vue'

/**
 * Props accepted by the main toolbar displayed above the current screen.
 */
interface Props {
  /**
   * Controlled search query value.
   */
  modelValue: string
  /**
   * Indicates whether the back button should be rendered.
   * @default false
   */
  showBackButton?: boolean
  /**
   * Indicates whether the parameters panel should be visible.
   * @default false
   */
  isParametersVisible?: boolean
  /**
   * Indicates whether the search bar should be visible.
   * @default false
   */
  isSearchVisible?: boolean
  /**
   * Available media type filter labels.
   * @default []
   */
  mediaTypeOptions?: SearchFilterOption[]
  /**
   * Selected media type filter labels.
   * @default []
   */
  selectedMediaTypes?: string[]
  /**
   * Available theme filter labels.
   * @default []
   */
  themeOptions?: SearchFilterOption[]
  /**
   * Selected theme filter labels.
   * @default []
   */
  selectedThemes?: string[]
}

withDefaults(defineProps<Props>(), {
  showBackButton: false,
  isParametersVisible: false,
  isSearchVisible: false,
  mediaTypeOptions: () => [],
  selectedMediaTypes: () => [],
  themeOptions: () => [],
  selectedThemes: () => [],
})
const emit = defineEmits<{
  'update:modelValue': [value: string]
  'update:selectedMediaTypes': [value: string[]]
  'update:selectedThemes': [value: string[]]
  submit: []
  back: []
  home: []
  lives: []
  'toggle-parameters': []
  'close-parameters': []
  'toggle-search': []
}>()

const parametersPanelId = useId()
const parametersButtonId = useId()

/**
 * Focuses the search input inside the search bar.
 * Only works when the search bar is visible.
 */
function focusSearchInput() {
  // Forward to the toolbar's SearchBar via DOM query (simple and reliable)
  const searchInput = document.querySelector('.main-bar__search .search-bar__input') as HTMLInputElement | null
  searchInput?.focus()
}

defineExpose({ focusSearchInput })
</script>

<template>
  <header class="main-bar">
    <MainBarToolbar
      :model-value="modelValue"
      :show-back-button="showBackButton"
      :is-parameters-visible="isParametersVisible"
      :is-search-visible="isSearchVisible"
      :parameters-button-id="parametersButtonId"
      :parameters-panel-id="parametersPanelId"
      :media-type-options="mediaTypeOptions"
      :selected-media-types="selectedMediaTypes"
      :theme-options="themeOptions"
      :selected-themes="selectedThemes"
      @update:model-value="emit('update:modelValue', $event)"
      @update:selected-media-types="emit('update:selectedMediaTypes', $event)"
      @update:selected-themes="emit('update:selectedThemes', $event)"
      @submit="emit('submit')"
      @back="emit('back')"
      @home="emit('home')"
      @lives="emit('lives')"
      @toggle-parameters="emit('toggle-parameters')"
      @toggle-search="emit('toggle-search')"
    />

    <MainBarParametersPopover
      :panel-id="parametersPanelId"
      :is-parameters-visible="isParametersVisible"
      :toggle-button-id="parametersButtonId"
      @close-parameters="emit('close-parameters')"
    />
  </header>
</template>

<style scoped>
.main-bar {
  position: sticky;
  top: 0;
  z-index: 10;
  isolation: isolate;
  display: grid;
  gap: 14px;
  padding-top: 2px;
  padding-bottom: 6px;
  background: var(--bg-transparent);
}

.main-bar::before {
  content: '';
  position: absolute;
  inset: 0 auto 0 50%;
  z-index: 0;
  width: 100vw;
  transform: translateX(-50%);
  background: var(--bg-surface);
  backdrop-filter: var(--backdrop-filter-medium);
  -webkit-backdrop-filter: var(--backdrop-filter-medium);
  pointer-events: none;
}

@media (max-width: 640px) {
  .main-bar {
    gap: 12px;
    padding-top: 2px;
  }
}
</style>
