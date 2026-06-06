<script setup lang="ts">
import type { MediaItem, ThumbnailOrientation } from '@/types/media'

import MediaCardActionButton from './MediaCardActionButton.vue'
import MediaCardDetailsContent from './MediaCardDetailsContent.vue'

/**
 * Props accepted by the interactive preview panel rendered over one card layout.
 */
interface Props {
  /**
   * Media item rendered by the card.
   */
  item: MediaItem
  /**
   * Stable title shown when the backend title is missing.
   */
  displayTitle: string
  /**
   * Indicates whether the card can request the detailed entry.
   */
  canSelectItem: boolean
  /**
   * Indicates whether the preview should currently be visible.
   */
  isPreviewOpen: boolean
  /**
   * Thumbnail orientation applied to the card layout.
   */
  thumbnailOrientation: ThumbnailOrientation
  /**
   * Display title for the backend service attached to this item.
   */
  serviceTitle: string | null
}

defineProps<Props>()

defineEmits<{
  select: []
}>()
</script>

<template>
  <aside
    class="media-card__preview"
    :class="{ 'media-card__preview--open': isPreviewOpen }"
    :aria-hidden="!isPreviewOpen"
  >
    <div
      class="media-card__preview-copy"
      :class="{ 'media-card__preview-copy--landscape': thumbnailOrientation === 'landscape' }"
    >
      <div class="media-card__preview-body">
        <h3 class="media-card__preview-title">
          {{ displayTitle }}
        </h3>

        <MediaCardDetailsContent :item="item" :service-title="serviceTitle" />
      </div>

      <MediaCardActionButton
        :can-select-item="canSelectItem"
        @select="$emit('select')"
      />
    </div>
  </aside>
</template>

<style scoped>
.media-card__preview {
  position: absolute;
  inset: 0;
  z-index: 2;
  display: flex;
  opacity: 0;
  pointer-events: none;
  transform: scale(0.985);
  transition:
    opacity 170ms ease,
    transform 170ms ease;
  background: var(--bg-overlay-media-card-preview);
  backdrop-filter: var(--backdrop-filter-medium);
}

.media-card__preview--open {
  opacity: 1;
  pointer-events: auto;
  transform: scale(1);
}

.media-card__preview-copy {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  gap: 12px;
  padding: 15px 14px 14px;
}

.media-card__preview-copy--landscape {
  padding: 18px 18px 16px;
}

.media-card__preview-body {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 8px;
  justify-content: flex-start;
  overflow: auto;
  overscroll-behavior: contain;
}

.media-card__preview-title {
  display: block;
  overflow: visible;
  color: var(--text-primary);
  font-size: 1.1rem;
  font-weight: 800;
  line-height: 1.2;
}
</style>
