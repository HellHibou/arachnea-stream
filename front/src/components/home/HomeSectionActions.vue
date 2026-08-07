<script setup lang="ts">
import { computed, useId } from 'vue'

import type { HomeSection } from '@/types/home'
import type { MediaCardCollectionMode, ThumbnailImageFit, ThumbnailOrientation } from '@/types/media'
import { useI18n } from '@/i18n'
import ParameterSegmented from '@/components/parameters/ParameterSegmented.vue'

/**
 * Props accepted by the home section action toolbar.
 */
interface Props {
  /**
   * Section controlled by the action buttons.
   */
  section: HomeSection
  /**
   * Whether the section is currently pinned.
   */
  isPinned: boolean
  /**
   * Whether the pinned section can move upward.
   */
  canMoveUp: boolean
  /**
   * Whether the pinned section can move downward.
   */
  canMoveDown: boolean
  /**
   * Current thumbnail orientation for the section.
   */
  thumbnailOrientation: ThumbnailOrientation
  /**
   * Current thumbnail image fit for the section.
   */
  thumbnailImageFit: ThumbnailImageFit
  /**
   * Current collection mode for the section.
   */
  collectionMode: MediaCardCollectionMode
}

/** Component props without defaults. */
const props = defineProps<Props>()

const emit = defineEmits<{
  /** Emitted when a section pinned state should be toggled. */
  'toggle-pinned': [section: HomeSection]
  /** Emitted when a pinned section should be moved. */
  'move-pinned': [section: HomeSection, direction: 'up' | 'down']
  /** Emitted when a section thumbnail orientation should be updated. */
  'update-thumbnail-orientation': [sectionPreferenceKey: string, orientation: ThumbnailOrientation | null]
  /** Emitted when a section thumbnail image fit should be updated. */
  'update-thumbnail-image-fit': [sectionPreferenceKey: string, imageFit: ThumbnailImageFit | null]
  /** Emitted when a section collection mode should be updated. */
  'update-collection-mode': [sectionPreferenceKey: string, mode: MediaCardCollectionMode | null]
}>()

/** Unique component identifier for generating element IDs. */
const componentId = useId()
/** Internationalization utilities. */
const { t } = useI18n()
/** Available options for card collection mode selection. */
const collectionModeOptions = computed<Array<{ value: MediaCardCollectionMode; label: string }>>(() => [
  { value: 'grid', label: t('layout.grid') },
  { value: 'single-row', label: t('layout.row') },
  { value: 'list', label: t('layout.list') },
])
const thumbnailOrientationOptions = computed<Array<{ value: ThumbnailOrientation; label: string }>>(() => [
  { value: 'landscape', label: t('settings.landscape') },
  { value: 'portrait', label: t('settings.portrait') },
])
const thumbnailImageFitOptions = computed<Array<{ value: ThumbnailImageFit; label: string }>>(() => [
  { value: 'contain', label: t('settings.complete') },
  { value: 'cover', label: t('settings.cropped') },
])
</script>

<template>
  <div class="home-section-actions">
    <button
      class="home-section-actions__button"
      :aria-label="props.isPinned ? t('catalog.unpin') : t('catalog.pin')"
      :aria-pressed="props.isPinned"
      :title="props.isPinned ? t('catalog.unpin') : t('catalog.pin')"
      type="button"
      @click="emit('toggle-pinned', props.section)"
    >
      <v-icon
        :icon="props.isPinned ? 'mdi-pin' : 'mdi-pin-outline'"
        size="18"
        aria-hidden="true"
      />
    </button>

    <button
      v-if="props.isPinned"
      class="home-section-actions__button"
      :aria-label="t('catalog.moveUp')"
      :title="t('catalog.moveUp')"
      type="button"
      :disabled="!props.canMoveUp"
      @click="emit('move-pinned', props.section, 'up')"
    >
      <v-icon icon="mdi-arrow-up" size="18" aria-hidden="true" />
    </button>

    <button
      v-if="props.isPinned"
      class="home-section-actions__button"
      :aria-label="t('catalog.moveDown')"
      :title="t('catalog.moveDown')"
      type="button"
      :disabled="!props.canMoveDown"
      @click="emit('move-pinned', props.section, 'down')"
    >
      <v-icon icon="mdi-arrow-down" size="18" aria-hidden="true" />
    </button>

    <v-menu
      v-if="props.isPinned"
      :close-on-content-click="false"
      :nudge-right="40"
      offset-y
      min-width="320px"
      max-width="400px"
    >
      <template #activator="{ props: activatorProps }">
        <button
          v-bind="activatorProps"
          class="home-section-actions__button"
          :aria-label="t('catalog.displayOptions')"
          :title="t('catalog.displayOptions')"
          type="button"
        >
          <v-icon icon="mdi-tune" size="18" aria-hidden="true" />
        </button>
      </template>

      <v-list class="home-section-actions__menu-list">
        <v-list-item-title class="home-section-actions__menu-title">
          {{ t('settings.cardLayout') }}
        </v-list-item-title>
        <ParameterSegmented
          :input-name="`${componentId}-collection-mode`"
          :label="t('settings.cardLayout')"
          :model-value="props.collectionMode"
          :options="collectionModeOptions"
          @update:model-value="(value: string | number | boolean) => emit('update-collection-mode', props.section.preferenceKey, value as MediaCardCollectionMode)"
        />

        <v-divider class="home-section-actions__divider" />

        <v-list-item-title class="home-section-actions__menu-title">
          {{ t('settings.thumbnailFormat') }}
        </v-list-item-title>
        <div class="home-section-actions__segmented">
          <ParameterSegmented
            :input-name="`${componentId}-thumbnail-orientation`"
            :label="t('settings.thumbnailFormat')"
            :model-value="props.thumbnailOrientation"
            :options="thumbnailOrientationOptions"
            @update:model-value="(value: string | number | boolean) => emit('update-thumbnail-orientation', props.section.preferenceKey, value as ThumbnailOrientation | null)"
          />
        </div>

        <v-divider class="home-section-actions__divider" />

        <v-list-item-title class="home-section-actions__menu-title">
          {{ t('settings.imageDisplay') }}
        </v-list-item-title>
        <div class="home-section-actions__segmented">
          <ParameterSegmented
            :input-name="`${componentId}-thumbnail-image-fit`"
            :label="t('settings.imageDisplay')"
            :model-value="props.thumbnailImageFit"
            :options="thumbnailImageFitOptions"
            @update:model-value="(value: string | number | boolean) => emit('update-thumbnail-image-fit', props.section.preferenceKey, value as ThumbnailImageFit | null)"
          />
        </div>
      </v-list>
    </v-menu>
  </div>
</template>

<style scoped>
.home-section-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.home-section-actions__button {
  display: inline-grid;
  place-items: center;
  width: 36px;
  min-width: 36px;
  height: 36px;
  padding: 0;
  border: 1px solid var(--border-color-primary);
  border-radius: 999px;
  background: var(--bg-surface);
  color: var(--text-secondary);
  font: inherit;
  cursor: pointer;
  box-shadow: var(--inset-light);
  transition:
    transform var(--duration-fast) ease,
    border-color var(--duration-fast) ease,
    background-color var(--duration-fast) ease,
    color var(--duration-fast) ease;
}

.home-section-actions__button:hover:not(:disabled) {
  transform: translateY(-1px);
  border-color: var(--color-primary);
  color: var(--text-primary);
}

.home-section-actions__button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.home-section-actions__button:disabled {
  cursor: default;
  opacity: 0.42;
}

.home-section-actions__button[aria-pressed='true'] {
  border-color: var(--color-primary);
  background:
    linear-gradient(180deg, rgba(86, 171, 255, 0.18), rgba(86, 171, 255, 0.08)),
    var(--bg-surface);
  color: var(--text-primary);
}

.home-section-actions__menu-list {
  gap: 0 !important;
  padding: 4px 0 !important;
  background: var(--bg-surface-strong);
  backdrop-filter: var(--backdrop-filter-soft);
  box-shadow: var(--shadow-heavy), var(--inset-light) !important;
}

.home-section-actions__menu-title {
  position: relative;
  margin-bottom: 2px !important;
  padding: 0 12px !important;
  font-size: 0.85rem !important;
  font-weight: 600 !important;
}

.home-section-actions__menu-title::before {
  content: '';
  position: absolute;
  top: 50%;
  left: 4px;
  width: 4px;
  height: 18px;
  border-radius: 999px;
  transform: translateY(-50%);
  background: var(--bg-accent-marker-primary);
}

.home-section-actions__radio-group {
  gap: 4px !important;
  margin-bottom: 0 !important;
  padding: 0 12px !important;
}

.home-section-actions__radio-group :deep(.v-radio) {
  min-width: 20px !important;
}

.home-section-actions__divider {
  margin: 4px 0 !important;
}

@media (max-width: 640px) {
  .home-section-actions {
    gap: 6px;
  }

  .home-section-actions__button {
    width: 34px;
    min-width: 34px;
    height: 34px;
  }
}
</style>
