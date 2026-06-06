<script setup lang="ts">
import type { SearchFilterOption } from '@/services/rustify'
import { useI18n } from '@/i18n'

import SearchBar from '../SearchBar.vue'

/**
 * Props accepted by the main toolbar row rendered above the current screen.
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
   * Identifier applied to the parameters toggle button.
   */
  parametersButtonId: string
  /**
   * Identifier of the associated parameters popover panel.
   */
  parametersPanelId: string
  /**
   * Available media type filter labels.
   */
  mediaTypeOptions: SearchFilterOption[]
  /**
   * Controlled selected media type labels.
   */
  selectedMediaTypes: string[]
  /**
   * Available theme filter labels.
   */
  themeOptions: SearchFilterOption[]
  /**
   * Controlled selected theme labels.
   */
  selectedThemes: string[]
}

withDefaults(defineProps<Props>(), {
  showBackButton: false,
  isParametersVisible: false,
  isSearchVisible: false,
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
  'toggle-search': []
}>()

const { t } = useI18n()
</script>

<template>
  <div class="main-bar__row">
    <div class="main-bar__leading">
      <button
        class="main-bar__back-button"
        :class="{ 'main-bar__back-button--hidden': !showBackButton }"
        type="button"
        :tabindex="showBackButton ? 0 : -1"
        :aria-hidden="!showBackButton"
        :aria-label="t('toolbar.back')"
        @click="emit('back')"
      >
        <v-icon icon="$NavigateBefore" size="22" aria-hidden="true" />
      </button>

      <button
        class="main-bar__home-button"
        type="button"
        :aria-label="t('toolbar.home')"
        @click="emit('home')"
      >
        <v-icon icon="mdi-home-outline" size="20" aria-hidden="true" />
      </button>
      <div>
        <img src="/logo.png" alt="" class="logo" />
        <span class="main-bar__brand">
          Arachnéa
        </span>
      </div>
    </div>

    <div class="main-bar__search-group">
      <SearchBar
        v-show="isSearchVisible"
        class="main-bar__search"
        :model-value="modelValue"
        :media-type-options="mediaTypeOptions"
        :selected-media-types="selectedMediaTypes"
        :theme-options="themeOptions"
        :selected-themes="selectedThemes"
        :button-label="t('search.button')"
        :placeholder="t('search.placeholder')"
        @update:model-value="emit('update:modelValue', $event)"
        @update:selected-media-types="emit('update:selectedMediaTypes', $event)"
        @update:selected-themes="emit('update:selectedThemes', $event)"
        @submit="emit('submit')"
      />
    </div>

    <div class="main-bar__actions">
      <button
        class="main-bar__lives-button"
        type="button"
        :aria-label="t('toolbar.lives')"
        @click="emit('lives')"
      >
        <v-icon icon="mdi-television-play" size="22" aria-hidden="true" />
      </button>

      <button
        class="main-bar__search-toggle-button"
        type="button"
        :aria-expanded="isSearchVisible"
        :aria-label="isSearchVisible ? t('toolbar.hideSearch') : t('toolbar.showSearch')"
        @click="emit('toggle-search')"
      >
        <v-icon icon="$Search" size="22" aria-hidden="true" />
      </button>

      <button
        class="main-bar__parameters-button"
        type="button"
        :id="parametersButtonId"
        :aria-controls="parametersPanelId"
        :aria-expanded="isParametersVisible"
        :aria-label="isParametersVisible ? t('toolbar.hideSettings') : t('toolbar.showSettings')"
        @click="emit('toggle-parameters')"
      >
        <v-icon icon="mdi-cog" size="22" aria-hidden="true" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.logo  {
  width: 32px;
  height: 32px;
  position: relative;
  top: 5px;
}
.main-bar__row {
  position: relative;
  z-index: 2;
  display: grid;
  align-items: center;
  grid-template-columns: minmax(0, auto) minmax(0, 1fr) auto;
  gap: 16px;
}

.main-bar__leading {
  display: grid;
  grid-template-columns: 52px 52px minmax(0, auto);
  align-items: center;
  gap: 12px;
  min-width: 0;
}

.main-bar__search-group {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}

.main-bar__search {
  min-width: 0;
  flex: 1 1 auto;
}

.main-bar__actions {
  display: flex;
  align-items: center;
  justify-content: end;
  gap: 12px;
}

.main-bar__brand {
  position:relative;
  top: -3px;
  min-width: 0;
  justify-self: center;
  color: var(--text-primary);
  font-size: 1.5rem;
  font-weight: 800;
  letter-spacing: 0.02em;
  line-height: 1.1;
  text-align: center;
  text-shadow: var(--text-shadow-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.main-bar__back-button,
.main-bar__home-button,
.main-bar__lives-button,
.main-bar__parameters-button,
.main-bar__search-toggle-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 52px;
  min-height: var(--control-height);
  padding: 0;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
  font-size: 0.96rem;
  font-weight: 700;
  cursor: pointer;
  box-shadow:
    var(--shadow-heavy),
    var(--inset-light);
  transition:
    transform var(--duration-fast) ease,
    border-color var(--duration-fast) ease,
    background-color var(--duration-fast) ease,
    box-shadow var(--duration-fast) ease;
}

.main-bar__back-button--hidden {
  visibility: hidden;
  pointer-events: none;
}

.main-bar__back-button:hover,
.main-bar__home-button:hover,
.main-bar__lives-button:hover,
.main-bar__parameters-button:hover,
.main-bar__search-toggle-button:hover {
  transform: translateY(-1px);
  border-color: var(--color-primary);
  background: var(--bg-surface);
}

.main-bar__back-button:focus-visible,
.main-bar__home-button:focus-visible,
.main-bar__lives-button:focus-visible,
.main-bar__parameters-button:focus-visible,
.main-bar__search-toggle-button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.main-bar__search-toggle-button[aria-expanded='true'],
.main-bar__parameters-button[aria-expanded='true'] {
  border-color: var(--color-primary);
  background: var(--bg-surface);
  box-shadow:
    var(--shadow-heavy),
    0 0 0 1px var(--color-primary);
}

@media (max-width: 1080px) {
  .main-bar__row {
    grid-template-columns: minmax(0, auto) minmax(0, 1fr) auto;
    grid-template-rows: auto auto;
    gap: 0 16px;
  }

  .main-bar__search-group {
    grid-column: 1 / -1;
    grid-row: 2;
    min-width: 0;
  }
}

@media (max-width: 820px) {
  .main-bar__row {
    grid-template-columns: auto auto;
    grid-template-rows: auto auto;
    align-items: start;
  }

  .main-bar__leading {
    grid-column: 1;
    grid-row: 1;
    grid-template-columns: 48px 48px minmax(0, auto);
  }

  .main-bar__search-group {
    grid-column: 1 / -1;
    grid-row: 2;
    min-width: 0;
  }

  .main-bar__actions {
    grid-column: 2;
    grid-row: 1;
  }
}

@media (max-width: 640px) {
  .main-bar__row {
    grid-template-columns: auto auto;
    grid-template-rows: auto auto;
  }

  .main-bar__back-button,
  .main-bar__home-button,
  .main-bar__lives-button,
  .main-bar__parameters-button,
  .main-bar__search-toggle-button {
    min-height: var(--control-height);
  }

  .main-bar__leading {
    grid-column: 1;
    grid-row: 1;
    grid-template-columns: 48px 48px minmax(0, auto);
  }

  .main-bar__search-group {
    gap: 10px;
    grid-column: 1 / -1;
    grid-row: 2;
  }

  .main-bar__actions {
    grid-column: 2;
    grid-row: 1;
    justify-self: end;
  }

  .main-bar__back-button,
  .main-bar__home-button,
  .main-bar__lives-button,
  .main-bar__search-toggle-button {
    min-width: 48px;
  }

  .main-bar__search-group,
  .main-bar__actions {
    grid-column: 1 / -1;
  }

}
</style>
