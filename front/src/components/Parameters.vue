<script setup lang="ts">
import { computed, useId } from 'vue'

import type { MediaCardCollectionMode } from '@/types/media'
import {
  type HomePreferences,
  type Parameters as StorageParameters,
  useStorage,
} from '@/services/storage'
import { useI18n } from '@/i18n'
import { useTheme } from '@/composables/useTheme'

import ParameterSegmented from './parameters/ParameterSegmented.vue'
import ParameterSwitch from './parameters/ParameterSwitch.vue'
import ParameterThemeSelector from './parameters/ParameterThemeSelector.vue'
import ParametersPanel from './parameters/ParametersPanel.vue'
import ParametersSection from './parameters/ParametersSection.vue'

/** Application parameters from persistent storage. */
const parameters: StorageParameters = useStorage().getParameters()
/** Home preferences from persistent storage. */
const homePreferences: HomePreferences = useStorage().getHomePreferences()
/** Internationalization utilities and current language options. */
const { languageOptions, selectedLanguage, setLanguage, t } = useI18n()
/** Theme management composable for switching color presets. */
const { currentPreset, presets, setTheme, isVisible } = useTheme()
/** Unique component identifier for generating element IDs. */
const componentId = useId()
/** Unique ID for the language select element. */
const languageInputId = `${componentId}-language`
/** Unique ID for the thumbnail orientation switch. */
const thumbnailOrientationInputId = `${componentId}-thumbnail-orientation`
/** Unique ID for the thumbnail image fit switch. */
const thumbnailImageFitInputId = `${componentId}-thumbnail-image-fit`
/** Unique name for the collection mode segmented control. */
const collectionModeInputName = `${componentId}-collection-mode`
/** Unique name for the favorite collection mode segmented control. */
const favoriteCollectionModeInputName = `${componentId}-favorite-collection-mode`
/** Unique ID for the show section editing buttons switch. */
const showSectionEditingButtonsInputId = `${componentId}-show-section-editing-buttons`
/** Unique ID for the background animation switch. */
const backgroundAnimationInputId = `${componentId}-background-animation`
/** Unique ID for the background image fit switch. */
const backgroundImageFitInputId = `${componentId}-background-image-fit`
/** Unique ID for the use trailer as background switch. */
const useTrailerAsBackgroundInputId = `${componentId}-use-trailer-as-background`
/** Unique ID for the use catalog banners as background switch. */
const useCatalogBannersAsBackgroundInputId = `${componentId}-use-catalog-banners-as-background`
/** Unique name for the theme segmented control. */
const themeInputName = `${componentId}-theme`
/** Available options for card collection mode selection. */
const cardCollectionModeOptions = computed<Array<{ value: MediaCardCollectionMode; label: string }>>(() => [
  { value: 'grid', label: t('layout.grid') },
  { value: 'single-row', label: t('layout.row') },
  { value: 'list', label: t('layout.list') },
])
/** Collection mode type excluding single-row for search. */
type SearchCollectionMode = Exclude<MediaCardCollectionMode, 'single-row'>
/** Available options for search collection mode selection. */
const searchCollectionModeOptions = computed<Array<{ value: SearchCollectionMode; label: string }>>(() => [
  { value: 'grid', label: t('layout.grid') },
  { value: 'list', label: t('layout.list') },
])
/** Available theme preset options for the theme selector. */
const themeOptions = computed<Array<{ value: string; label: string; color: string }>>(() =>
  presets.map((preset) => ({
    value: preset.key,
    label: preset.label,
    color: `hsl(${preset.h}, ${preset.s}, ${preset.l})`,
  })),
)
/** Current theme key derived from parameters. */
const selectedTheme = computed<string>(() => parameters.theme.value)
/** Whether the language selector should be displayed. */
const isLanguageVisible = computed(() => languageOptions.value.length > 2)
/** Current search collection mode derived from parameters. */
const searchCollectionMode = computed<SearchCollectionMode>(() =>
  parameters.collectionMode.value === 'list' ? 'list' : 'grid',
)

/**
 * Forwards the thumbnail orientation selected by the switch.
 *
 * @param checked Checked state reported by the reusable switch component.
 */
function handleThumbnailOrientationUpdate(checked: boolean) {
  parameters.thumbnailOrientation.value = checked ? 'landscape' : 'portrait'
}

/**
 * Forwards the thumbnail fit mode selected by the switch.
 *
 * @param checked Checked state reported by the reusable switch component.
 */
function handleThumbnailImageFitUpdate(checked: boolean) {
  parameters.thumbnailImageFit.value = checked ? 'contain' : 'cover'
}

/**
 * Forwards the result collection layout selected by the segmented control.
 *
 * @param value Newly selected collection mode.
 */
function handleCollectionModeUpdate(value: string) {
  if (value !== 'grid' && value !== 'list') {
    return
  }

  parameters.collectionMode.value = value
}

/**
 * Forwards the pinned home section layout selected by the segmented control.
 *
 * @param value Newly selected collection mode.
 */
function handleFavoriteCollectionModeUpdate(value: string) {
  homePreferences.favoriteCollectionMode.value = value as MediaCardCollectionMode
}

/**
 * Forwards the home editing button visibility selected by the switch.
 *
 * @param checked Checked state reported by the reusable switch component.
 */
function handleShowSectionEditingButtonsUpdate(checked: boolean) {
  homePreferences.showSectionEditingButtons.value = checked
}

/**
 * Forwards the background animation preference selected by the switch.
 *
 * @param checked Checked state reported by the reusable switch component.
 */
function handleBackgroundAnimationUpdate(checked: boolean) {
  parameters.isBackgroundAnimated.value = checked
}

/**
 * Forwards the background image fit preference selected by the switch.
 *
 * @param checked Checked state reported by the reusable switch component.
 */
function handleBackgroundImageFitUpdate(checked: boolean) {
  parameters.backgroundImageFit.value = checked ? 'contain' : 'cover'
}

/**
 * Forwards the trailer background preference selected by the switch.
 *
 * @param checked Checked state reported by the reusable switch component.
 */
function handleUseTrailerAsBackgroundUpdate(checked: boolean) {
  parameters.useTrailerAsBackground.value = checked
}

/**
 * Forwards the catalog banner background preference selected by the switch.
 *
 * @param checked Checked state reported by the reusable switch component.
 */
function handleUseCatalogBannersAsBackgroundUpdate(checked: boolean) {
  parameters.useCatalogBannersAsBackground.value = checked
}

/**
   * Persists the selected interface language.
   *
   * @param event Change event fired by the language select.
   */
  function handleLanguageUpdate(event: Event) {
    const target = event.target as HTMLSelectElement
    void setLanguage(target.value)
  }

  /**
   * Switches the active theme preset.
   *
   * @param key Theme preset key selected by the user.
   */
  function handleThemeUpdate(key: string) {
    setTheme(key)
  }
</script>

<template>
  <section class="parameters" :aria-label="t('settings.ariaLabel')">
    <ParametersSection :title="t('settings.general')">
      <div class="parameters__grid">
        <ParametersPanel v-if="isLanguageVisible" :title="t('settings.language.panel')">
          <label class="parameters__select-field" :for="languageInputId">
            <span class="parameters__select-label">{{ t('settings.language.label') }}</span>
            <select
              :id="languageInputId"
              class="parameters__select"
              :value="selectedLanguage ?? 'auto'"
              @change="handleLanguageUpdate"
            >
              <option
                v-for="language in languageOptions"
                :key="language.code"
                :value="language.code"
              >
                {{ language.label }}
              </option>
            </select>
          </label>
        </ParametersPanel>

        <ParametersPanel v-if="isVisible" :title="t('settings.colorTheme')">
          <ParameterThemeSelector
            :input-name="themeInputName"
            :model-value="selectedTheme"
            :options="themeOptions"
            @update:model-value="handleThemeUpdate"
          />
        </ParametersPanel>

        <ParametersPanel :title="t('settings.thumbnailFormat')">
          <ParameterSwitch
            :input-id="thumbnailOrientationInputId"
            :leading-label="t('settings.portrait')"
            :trailing-label="t('settings.landscape')"
            :checked="parameters.thumbnailOrientation.value === 'landscape'"
            @update:checked="handleThumbnailOrientationUpdate"
          />
        </ParametersPanel>

        <ParametersPanel :title="t('settings.imageDisplay')">
          <ParameterSwitch
            :input-id="thumbnailImageFitInputId"
            :leading-label="t('settings.cropped')"
            :trailing-label="t('settings.complete')"
            :checked="parameters.thumbnailImageFit.value === 'contain'"
            @update:checked="handleThumbnailImageFitUpdate"
          />
        </ParametersPanel>
      </div>
    </ParametersSection>

    <ParametersSection :title="t('settings.home')">
      <div class="parameters__grid">
        <ParametersPanel :title="t('settings.editingButtons')">
          <ParameterSwitch
            :input-id="showSectionEditingButtonsInputId"
            :leading-label="t('settings.hidden')"
            :trailing-label="t('settings.visible')"
            :checked="homePreferences.showSectionEditingButtons.value"
            @update:checked="handleShowSectionEditingButtonsUpdate"
          />
        </ParametersPanel>

        <ParametersPanel :title="t('settings.favoriteCardLayout')">
          <ParameterSegmented
            :input-name="favoriteCollectionModeInputName"
            :label="t('settings.favoriteCardLayout')"
            :model-value="homePreferences.favoriteCollectionMode.value"
            :options="cardCollectionModeOptions"
            @update:model-value="handleFavoriteCollectionModeUpdate"
          />
        </ParametersPanel>
      </div>
    </ParametersSection>

    <ParametersSection :title="t('settings.searchDisplay')">
      <div class="parameters__grid">
        <ParametersPanel :title="t('settings.cardLayout')">
          <ParameterSegmented
            :input-name="collectionModeInputName"
            :label="t('settings.cardLayout')"
            :model-value="searchCollectionMode"
            :options="searchCollectionModeOptions"
            @update:model-value="handleCollectionModeUpdate"
          />
        </ParametersPanel>
      </div>
    </ParametersSection>

<ParametersSection :title="t('settings.background')">
      <div class="parameters__grid">
         <ParametersPanel :title="t('settings.backgroundImage')">
           <ParameterSwitch
             :input-id="useCatalogBannersAsBackgroundInputId"
             :leading-label="t('settings.inactive')"
             :trailing-label="t('settings.active')"
             :checked="parameters.useCatalogBannersAsBackground.value"
             @update:checked="handleUseCatalogBannersAsBackgroundUpdate"
           />
         </ParametersPanel>

         <ParametersPanel :title="t('settings.backgroundAnimation')" :disabled="!parameters.useCatalogBannersAsBackground.value">
           <ParameterSwitch
             :input-id="backgroundAnimationInputId"
             :leading-label="t('settings.fixed')"
             :trailing-label="t('settings.animated')"
             :checked="parameters.isBackgroundAnimated.value"
             :disabled="!parameters.useCatalogBannersAsBackground.value"
             @update:checked="handleBackgroundAnimationUpdate"
           />
         </ParametersPanel>

         <ParametersPanel
           :title="t('settings.imageDisplay')"
           :disabled="parameters.isBackgroundAnimated.value || !parameters.useCatalogBannersAsBackground.value"
         >
           <ParameterSwitch
             :input-id="backgroundImageFitInputId"
             :leading-label="t('settings.cropped')"
             :trailing-label="t('settings.complete')"
             :checked="parameters.backgroundImageFit.value === 'contain'"
             :disabled="parameters.isBackgroundAnimated.value || !parameters.useCatalogBannersAsBackground.value"
             @update:checked="handleBackgroundImageFitUpdate"
           />
         </ParametersPanel>

         <ParametersPanel :title="t('settings.trailerUsage')">
           <ParameterSwitch
             :input-id="useTrailerAsBackgroundInputId"
             :leading-label="t('settings.image')"
             :trailing-label="t('settings.trailer')"
             :checked="parameters.useTrailerAsBackground.value"
             @update:checked="handleUseTrailerAsBackgroundUpdate"
           />
         </ParametersPanel>
       </div>
     </ParametersSection>
  </section>
</template>

<style scoped>
.parameters {
  display: grid;
  gap: 0px;
  max-height: calc(100vh - 90px);
  overflow-y: auto;
}

.parameters__grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 0;
}

.parameters__select-field {
  width: min(100%, var(--parameters-control-width));
}

.parameters__select-label {
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

.parameters__select {
  width: 100%;
  min-height: 40px;
  padding: 8px 14px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
}

.parameters__select:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>
