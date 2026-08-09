<script setup lang="ts">
import { toRef, useTemplateRef, watch } from 'vue'

import type { MediaItem, ThumbnailOrientation } from '@/types/media'
import { useMediaCardPreviewPosition } from '@/composables/media-card/useMediaCardPreviewPosition'

import MediaCardDetailsContent from './MediaCardDetailsContent.vue'

/**
 * Props accepted by the interactive preview popup rendered next to one card layout.
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
  /**
   * Root element of the card used as the popup anchor.
   */
  anchorRef: HTMLElement | null
}

/** Component props without defaults. */
const props = defineProps<Props>()

const emit = defineEmits<{
  /** Emitted when the popup root element changes. */
  popupRootChange: [element: HTMLElement | null]
}>()

/** Template reference to the popup root element. */
const popupRef = useTemplateRef<HTMLElement>('popup')

// Keep the collection manager informed about the popup root element.
watch(popupRef, (element) => {
  emit('popupRootChange', element)
}, { immediate: true })

/** Reactive reference to the anchor card element. */
const anchor = toRef(props, 'anchorRef')

/** Popup position state computed from the anchor and popup measurements. */
const { position } = useMediaCardPreviewPosition({
  anchorRef: anchor,
  popupRef,
  isOpen: () => props.isPreviewOpen,
})
</script>

<template>
  <Teleport to="body">
    <div
      v-if="isPreviewOpen"
      ref="popup"
      class="media-card-preview-popup"
      :class="`media-card-preview-popup--${position.placement}`"
      :style="{
        left: `${position.left ?? 0}px`,
        top: `${position.top ?? 0}px`,
        visibility: position.visible ? 'visible' : 'hidden',
      }"
      role="tooltip"
    >
      <!-- Small arrow pointing back to the anchor card. -->
      <span class="media-card-preview-popup__arrow" aria-hidden="true" />
      <div
        class="media-card-preview-popup__copy"
        :class="{ 'media-card-preview-popup__copy--landscape': thumbnailOrientation === 'landscape' }"
      >
        <h3 class="media-card-preview-popup__title">
          {{ displayTitle }}
        </h3>

        <MediaCardDetailsContent :item="item" :service-title="serviceTitle" />
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.media-card-preview-popup {
  position: fixed;
  z-index: 50;
  width: fit-content;
  max-width: min(640px, calc(100vw - 16px));
  max-height: min(480px, calc(100vh - 16px));
  overflow: auto;
  overscroll-behavior: contain;
  /* Keep internal scrolling available but hide both scrollbars. */
  scrollbar-width: none;
  /* The popup is display-only: it must not keep itself open nor intercept card interactions. */
  pointer-events: none;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface-strong);
  box-shadow: var(--shadow-heavy);
  backdrop-filter: var(--backdrop-filter-medium);
  color: var(--text-primary);
}

.media-card-preview-popup::-webkit-scrollbar {
  display: none;
}

.media-card-preview-popup__copy {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 15px 14px 14px;
}

.media-card-preview-popup__copy--landscape {
  padding: 18px 18px 16px;
}

/* Tooltip arrow centered on the edge facing the anchor card. */
.media-card-preview-popup__arrow {
  position: absolute;
  display: block;
  width: 10px;
  height: 10px;
  background: inherit;
  border: inherit;
  border-top: 0;
  border-right: 0;
}

.media-card-preview-popup--top .media-card-preview-popup__arrow {
  bottom: -5px;
  left: 50%;
  margin-left: -5px;
  transform: rotate(-45deg);
}

.media-card-preview-popup--bottom .media-card-preview-popup__arrow {
  top: -5px;
  left: 50%;
  margin-left: -5px;
  transform: rotate(135deg);
}

.media-card-preview-popup--left .media-card-preview-popup__arrow {
  top: 50%;
  right: -5px;
  margin-top: -5px;
  transform: rotate(-135deg);
}

.media-card-preview-popup--right .media-card-preview-popup__arrow {
  top: 50%;
  left: -5px;
  margin-top: -5px;
  transform: rotate(45deg);
}

.media-card-preview-popup__title {
  display: block;
  overflow: visible;
  color: var(--text-primary);
  font-size: 1.1rem;
  font-weight: 800;
  line-height: 1.2;
}
</style>