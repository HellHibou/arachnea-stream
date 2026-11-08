<script setup lang="ts">
import { computed, useId } from 'vue'

import type { MediaCardCollectionMode } from '@/types/media'
import {
  LANGUAGES,
  formatLanguageLabel,
} from '@/services/languages'
import {
  type HomePreferences,
  type Parameters as StorageParameters,
  useStorage,
} from '@/services/storage'
import { type SkipChapterType } from '@/composables/video/video-js-media-renderer/chapters'
import { useI18n } from '@/i18n'
import { useTheme } from '@/composables/useTheme'
import { isDesktopApp, openAdministrationWindow } from '@/services/rustify'

import ParameterSegmented from './parameters/ParameterSegmented.vue'
import ParameterSlider from './parameters/ParameterSlider.vue'
import ParameterSubtitleLanguages from './parameters/ParameterSubtitleLanguages.vue'
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
/** Unique ID for the embedded player mode select. */
const embeddedPlayerModeInputId = `${componentId}-embedded-player-mode`
/** Skippable chapter types whose autoskip switch stays in the parameters panel; the other types are configured from the player menu. */
const AUTOSKIP_PARAMETER_TYPES = ['ads'] as const satisfies readonly SkipChapterType[]

/** Unique input ID suffix per autoskip switch exposed in the parameters panel. */
const autoskipInputIdSuffix: Record<(typeof AUTOSKIP_PARAMETER_TYPES)[number], string> = {
  ads: 'autoskip-ads',
}
/** Collection mode type excluding single-row for search. */
type SearchCollectionMode = Exclude<MediaCardCollectionMode, 'single-row'>
/** Available options for search collection mode selection. */
const searchCollectionModeOptions = computed<Array<{ value: SearchCollectionMode; label: string }>>(() => [
  { value: 'grid', label: t('layout.grid') },
  { value: 'list', label: t('layout.list') },
])
/** Available thumbnail orientation options for the segmented control. */
const thumbnailOrientationOptions = computed<Array<{ value: 'portrait' | 'landscape'; label: string }>>(() => [
  { value: 'portrait', label: t('settings.portrait') },
  { value: 'landscape', label: t('settings.landscape') },
])
/** Available thumbnail image fit options for the segmented control. */
const thumbnailImageFitOptions = computed<Array<{ value: 'cover' | 'contain'; label: string }>>(() => [
  { value: 'cover', label: t('settings.cropped') },
  { value: 'contain', label: t('settings.complete') },
])
/** Available background animation options for the segmented control. */
const backgroundAnimationOptions = computed<Array<{ value: boolean; label: string }>>(() => [
  { value: false, label: t('settings.fixed') },
  { value: true, label: t('settings.animated') },
])
/** Available background image fit options for the segmented control. */
const backgroundImageFitOptions = computed<Array<{ value: 'cover' | 'contain'; label: string }>>(() => [
  { value: 'cover', label: t('settings.cropped') },
  { value: 'contain', label: t('settings.complete') },
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
/** Ordered track profiles applied to a new video source. */
const trackProfiles = computed(() => parameters['videoPlayer.trackProfiles'].value)
/** Whether audio-description tracks may be selected as a last resort. */
const allowAudioDescription = computed(() => parameters['videoPlayer.allowAudioDescription'].value)
/** Configured subtitle languages, highest priority first. */
const subtitleLanguages = computed(() => parameters['videoPlayer.subtitleLanguages'].value)
/** Audio language options offered in the profile select, derived from the shared catalog. */
const audioLanguageOptions = LANGUAGES.map((language) => ({
  value: language.code,
  label: formatLanguageLabel(language.code),
}))
/** Whether the dedicated administration window can be opened from this desktop app. */
const canOpenAdministration = isDesktopApp()

/** Whether the language selector should be displayed. */
const isLanguageVisible = computed(() => languageOptions.value.length > 2)
/** Current search collection mode derived from parameters. */
const searchCollectionMode = computed<SearchCollectionMode>(() =>
  parameters.collectionMode.value === 'list' ? 'list' : 'grid',
)

/**
 * Forwards the thumbnail orientation selected by the segmented control.
 *
 * @param value Newly selected thumbnail orientation.
 */
function handleThumbnailOrientationUpdate(value: string | number | boolean) {
  parameters.thumbnailOrientation.value = value as 'portrait' | 'landscape'
}

/**
 * Forwards the thumbnail fit mode selected by the segmented control.
 *
 * @param value Newly selected thumbnail fit mode.
 */
function handleThumbnailImageFitUpdate(value: string | number | boolean) {
  parameters.thumbnailImageFit.value = value as 'cover' | 'contain'
}

/**
 * Forwards the thumbnail size multiplier selected by the slider.
 *
 * @param value Newly selected thumbnail size multiplier.
 */
function handleThumbnailSizeMultiplierUpdate(value: number) {
  parameters.thumbnailSizeMultiplier.value = value
}

/**
 * Forwards the result collection layout selected by the segmented control.
 *
 * @param value Newly selected collection mode.
 */
function handleCollectionModeUpdate(value: string | number | boolean) {
  if (value !== 'grid' && value !== 'list') {
    return
  }

  parameters.collectionMode.value = value as 'grid' | 'list'
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
 * Forwards the background animation preference selected by the segmented control.
 *
 * @param value Newly selected background animation state.
 */
function handleBackgroundAnimationUpdate(value: string | number | boolean) {
  parameters.isBackgroundAnimated.value = value as boolean
}

/**
 * Forwards the background image fit preference selected by the segmented control.
 *
 * @param value Newly selected background image fit mode.
 */
function handleBackgroundImageFitUpdate(value: string | number | boolean) {
  parameters.backgroundImageFit.value = value as 'cover' | 'contain'
}

/**
 * Forwards the trailer background preference selected by the segmented control.
 *
 * @param value Newly selected trailer usage state.
 */
function handleUseTrailerAsBackgroundUpdate(value: string | number | boolean) {
  parameters.useTrailerAsBackground.value = value as boolean
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

/**
 * Persists the embedded player mode selected by the user.
 *
 * @param event Change event fired by the embedded player mode select.
 */
function handleEmbeddedPlayerModeUpdate(event: Event) {
  const target = event.target as HTMLSelectElement
  parameters.securityMode.value = target.value as 'unsafe' | 'confirmation' | 'safe'
}

/**
 * Persists the automatic chapter skip preference selected by the switch.
 *
 * @param chapterType Skippable chapter type owning the `videoPlayer.autoskip.*` parameter.
 * @param checked Checked state reported by the reusable switch component.
 */
function handleAutoskipUpdate(chapterType: SkipChapterType, checked: boolean) {
  parameters[`videoPlayer.autoskip.${chapterType}`].value = checked
}

/**
 * Returns the effective audio value for one profile.
 *
 * @param index Profile index.
 * @returns Audio target value or `null`.
 */
function resolveProfileAudio(index: number): string | null {
  return trackProfiles.value[index]?.audio ?? null
}

/**
 * Updates the audio language of one track profile.
 *
 * @param index Profile index.
 * @param value New audio target: `original`, a language code, or `null`.
 */
function handleProfileAudioUpdate(index: number, value: string | null) {
  const next = [...trackProfiles.value]
  if (!next[index]) {
    next[index] = { audio: null, subtitles: 'disabled' }
  }

  next[index].audio = value ?? null
  parameters['videoPlayer.trackProfiles'].value = next
}

/**
 * Updates the subtitle behavior of one track profile.
 *
 * @param index Profile index.
 * @param value New subtitle mode: `disabled` or `auto`.
 */
function handleProfileSubtitlesUpdate(index: number, value: 'disabled' | 'auto') {
  const next = [...trackProfiles.value]
  if (!next[index]) {
    next[index] = { audio: null, subtitles: 'disabled' }
  }

  next[index].subtitles = value
  parameters['videoPlayer.trackProfiles'].value = next
}

/**
 * Adds a new audio track profile at the end of the list.
 */
function handleAddAudioProfile(): void {
  const next = [...trackProfiles.value]
  next.push({ audio: null, subtitles: 'disabled' })
  parameters['videoPlayer.trackProfiles'].value = next
}

/**
 * Removes the audio track profile at the given index. The first profile
 * cannot be removed.
 *
 * @param index Profile index to remove.
 */
function handleRemoveAudioProfile(index: number): void {
  if (trackProfiles.value.length <= 1 || index === 0) {
    return
  }

  const next = [...trackProfiles.value]
  next.splice(index, 1)
  parameters['videoPlayer.trackProfiles'].value = next
}

/**
 * Persists the configured subtitle languages.
 *
 * @param value New ordered language code list.
 */
function handleSubtitleLanguagesUpdate(value: string[]): void {
  parameters['videoPlayer.subtitleLanguages'].value = value
}

/** Opens the dedicated administration window in the desktop application. */
function handleOpenAdministration(): void {
  void openAdministrationWindow()
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
          <ParameterSegmented
            :input-name="thumbnailOrientationInputId"
            :label="t('settings.thumbnailFormat')"
            :model-value="parameters.thumbnailOrientation.value"
            :options="thumbnailOrientationOptions"
            @update:model-value="handleThumbnailOrientationUpdate"
          />
        </ParametersPanel>
   
        <ParametersPanel :title="t('settings.thumbnailSize')">
          <ParameterSlider
            :label="t('settings.thumbnailSize')"
            :model-value="parameters.thumbnailSizeMultiplier.value"
            :min="0.5"
            :max="1.5"
            :step="0.10"
            :value-suffix="'×'"
            @update:model-value="handleThumbnailSizeMultiplierUpdate"
          />
        </ParametersPanel>

        <ParametersPanel :title="t('settings.imageDisplay')">
          <ParameterSegmented
            :input-name="thumbnailImageFitInputId"
            :label="t('settings.imageDisplay')"
            :model-value="parameters.thumbnailImageFit.value"
            :options="thumbnailImageFitOptions"
            @update:model-value="handleThumbnailImageFitUpdate"
          />
        </ParametersPanel>

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
      </div>
    </ParametersSection>

    <ParametersSection :title="t('settings.player')">
      <div class="parameters__grid">
        <ParametersPanel :title="t('settings.securityMode')">
          <label class="parameters__select-field" :for="embeddedPlayerModeInputId">
            <span class="parameters__select-label">{{ t('settings.securityMode') }}</span>
            <select
              :id="embeddedPlayerModeInputId"
              class="parameters__select"
              :value="parameters.securityMode.value"
              @change="handleEmbeddedPlayerModeUpdate"
            >
              <option
                v-for="mode in ['safe', 'confirmation', 'unsafe']"
                :key="mode"
                :value="mode"
              >
                {{ t(`settings.${mode}`) }}
              </option>
            </select>
          </label>
        </ParametersPanel>
        <ParametersPanel
          v-for="chapterType in AUTOSKIP_PARAMETER_TYPES"
          :key="chapterType"
          :title="t(`settings.autoskip.${chapterType}`)"
        >
          <ParameterSwitch
            :input-id="`${componentId}-${autoskipInputIdSuffix[chapterType]}`"
            :leading-label="t('settings.inactive')"
            :trailing-label="t('settings.active')"
            :checked="parameters[`videoPlayer.autoskip.${chapterType}`].value"
            @update:checked="(checked: boolean) => handleAutoskipUpdate(chapterType, checked)"
          />
        </ParametersPanel>

        <ParametersPanel>
          <div class="parameter-track-audio-group">
            <span class="parameter-track-audio__label">{{ t('settings.playerTracks.audio') }}</span>
            <div class="parameter-track-lines">
              <div
                v-for="(profile, index) in trackProfiles"
                :key="index"
                class="parameter-track-line"
              >
              <select
                class="parameter-track-line__select"
                :value="resolveProfileAudio(index)"
                @change="(event: Event) => {
                  handleProfileAudioUpdate(index, (event.target as HTMLSelectElement).value as string | null)
                }"
              >
                <option value="original">{{ t('settings.playerTracks.original') }}</option>
                <option
                  v-for="language in audioLanguageOptions"
                  :key="language.value"
                  :value="language.value"
                >
                  {{ language.label }}
                </option>
              </select>

              <div class="parameter-track-line__subtitles">
                <span class="parameter-track-line__subtitles-label">{{ t('settings.playerTracks.subtitles') }}</span>
                <ParameterSwitch
                  :input-id="`${componentId}-subtitles-${index}`"
                  :leading-label="t('settings.inactive')"
                  :trailing-label="t('settings.active')"
                  :checked="trackProfiles[index]?.subtitles === 'auto'"
                  @update:checked="(checked: boolean) => handleProfileSubtitlesUpdate(index, checked ? 'auto' : 'disabled')"
                />
              </div>

              <button
                type="button"
                class="parameter-track-line__button parameter-track-line__button--remove"
                :class="{ 'parameter-track-line__button--hidden': index === 0 }"
                @click="() => handleRemoveAudioProfile(index)"
                :aria-label="t('settings.removeAudioProfile')"
              >
                −
              </button>
              <button
                type="button"
                class="parameter-track-line__button parameter-track-line__button--add"
                :class="{ 'parameter-track-line__button--hidden': index !== trackProfiles.length - 1 }"
                @click="handleAddAudioProfile"
                :aria-label="t('settings.addAudioProfile')"
              >
                +
              </button>
            </div>
          </div>
          </div>
        </ParametersPanel>

        <ParametersPanel :title="t('settings.useAudioDescriptionIfAvailable')">
          <ParameterSwitch
            :input-id="`${componentId}-allow-audio-description`"
            :leading-label="t('settings.inactive')"
            :trailing-label="t('settings.active')"
            :checked="allowAudioDescription"
            @update:checked="(checked: boolean) => {
              parameters['videoPlayer.allowAudioDescription'].value = checked
            }"
          />
        </ParametersPanel>

        <ParametersPanel>
          <ParameterSubtitleLanguages
            :model-value="subtitleLanguages"
            :label="t('settings.subtitleLanguagesTitle')"
            @update:model-value="handleSubtitleLanguagesUpdate"
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
           <ParameterSegmented
             :input-name="backgroundAnimationInputId"
             :label="t('settings.backgroundAnimation')"
             :model-value="parameters.isBackgroundAnimated.value"
             :options="backgroundAnimationOptions"
             :disabled="!parameters.useCatalogBannersAsBackground.value"
             @update:model-value="handleBackgroundAnimationUpdate"
           />
         </ParametersPanel>

         <ParametersPanel
           :title="t('settings.imageDisplay')"
           :disabled="parameters.isBackgroundAnimated.value || !parameters.useCatalogBannersAsBackground.value"
         >
           <ParameterSegmented
             :input-name="backgroundImageFitInputId"
             :label="t('settings.imageDisplay')"
             :model-value="parameters.backgroundImageFit.value"
             :options="backgroundImageFitOptions"
             :disabled="parameters.isBackgroundAnimated.value || !parameters.useCatalogBannersAsBackground.value"
             @update:model-value="handleBackgroundImageFitUpdate"
           />
         </ParametersPanel>

         <ParametersPanel :title="t('settings.trailerUsage')">
           <ParameterSwitch
             :input-id="useTrailerAsBackgroundInputId"
             :leading-label="t('settings.inactive')"
             :trailing-label="t('settings.active')"
             :checked="parameters.useTrailerAsBackground.value"
             @update:checked="handleUseTrailerAsBackgroundUpdate"
           />
         </ParametersPanel>
       </div>
     </ParametersSection>

    <ParametersSection v-if="canOpenAdministration" :title="t('settings.administration')">
      <div class="parameters__grid">
        <ParametersPanel :title="t('settings.openAdministration')">
          <button
            type="button"
            class="parameters__action-button"
            @click="handleOpenAdministration"
          >
            {{ t('settings.openAdministration') }}
          </button>
        </ParametersPanel>
      </div>
    </ParametersSection>
  </section>
</template>

<style scoped>
.parameters__action-button {
  padding: 8px 12px;
  border: 1px solid var(--border-color-primary);
  border-radius: 8px;
  color: var(--text-primary);
  background: var(--bg-surface);
  cursor: pointer;
}

.parameters__action-button:hover,
.parameters__action-button:focus-visible {
  border-color: var(--color-primary);
}

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
  margin: 0px;
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

.parameter-track-audio-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 100%;
  flex: 1 1 100%;
}

.parameter-track-lines {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.parameter-track-line {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-left: 20px;
}

.parameter-track-line__button--hidden {
  visibility: hidden;
}

.parameter-track-line__select {
  flex: 1;
  min-width: 0;
  min-height: 40px;
}

.parameter-track-line__button {
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

.parameter-track-line__button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.parameter-track-line__button--remove {
  color: var(--text-danger);
  border-color: var(--border-color-danger);
}

.parameter-track-line__button--add {
  color: var(--text-primary);
  border-color: var(--border-color-primary);
}

.parameter-track-line__subtitles {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.parameter-track-line__subtitles-label {
  color: var(--text-primary);
  font-size: 0.92rem;
  font-weight: 600;
  white-space: nowrap;
  line-height: 30px;
}

.parameter-track-audio__label {
  display: block;
  color: var(--text-primary);
  font-size: 0.92rem;
  font-weight: 700;
  margin-bottom: 8px;
  white-space: nowrap;
  margin-left: 10px;
  line-height: 1.2;
}

@media (max-width: 720px) {
  .parameter-track-line {
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
  }

  .parameter-track-line__button {
    width: auto;
    padding: 0 10px;
  }
}
</style>
