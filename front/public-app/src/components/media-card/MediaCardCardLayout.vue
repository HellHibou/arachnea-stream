<script setup lang="ts">
import { ref, watch } from 'vue'

import type { MediaItem, ThumbnailImageFit, ThumbnailOrientation } from '@/types/media'

import { mediaCardPreview } from '@/composables/media-card/mediaCardPreview'

import MediaCardPoster from './MediaCardPoster.vue'
import MediaCardPreviewPanel from './MediaCardPreviewPanel.vue'

/**
 * Props accepted by the interactive card layout.
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
   * Formatted rating displayed in the poster badge.
   */
  formattedRating: string | null
  /**
   * CSS class applied to the rating badge.
   */
  ratingClass: string
  /**
   * Thumbnail orientation applied to the poster frame.
   */
  thumbnailOrientation: ThumbnailOrientation
  /**
   * Poster fit mode applied to the artwork.
   */
  thumbnailImageFit: ThumbnailImageFit
  /**
   * Indicates whether the poster image loaded successfully.
   */
  imageAvailable: boolean
  /**
   * Indicates whether the preview should currently be visible.
   */
  isPreviewOpen: boolean
  /**
   * Display title for the backend service attached to this item.
   */
  serviceTitle: string | null
  /**
   * Logo URL for the backend service attached to this item.
   */
  serviceLogo: string | null
  /**
   * Indicates whether the backend service logo should be displayed on the poster.
   */
  showServiceLogo: boolean
  /**
   * Internal route targeted by the card action, or null when no link is available.
   */
  href: string | null
}

/** Component props without defaults. */
const props = defineProps<Props>()

const emit = defineEmits<{
  /** Emitted when the poster image fails to load. */
  imageError: []
  /** Emitted when the service logo fails to load. */
  serviceLogoError: []
  /** Emitted when the card is selected. */
  select: []
  /** Emitted when the preview should be opened. */
  openPreview: []
  /** Emitted when the preview should be closed. */
  closePreview: []
  /** Emitted when the root element changes. */
  rootChange: [element: HTMLElement | null]
  /** Emitted when the popup root element changes. */
  popupRootChange: [element: HTMLElement | null]
}>()

/** Media card preview composable results. */
const {
  /** Template reference to the card element. */
  cardRef,
  /** Function to open the preview. */
  openPreview,
  /** Function to close the preview. */
  closePreview,
  /** Function to handle focus out events. */
  handleFocusOut,
} = mediaCardPreview({
  onOpen: () => {
    emit('openPreview')
  },
  onClose: () => {
    emit('closePreview')
  },
  onRootChange: (element) => {
    emit('rootChange', element)
  },
})

/** Anchor element passed to the preview popup, kept in sync after mounting. */
const anchorElement = ref<HTMLElement | null>(null)

// Sync the anchor element with the card reference once available (after mount).
watch(cardRef, (element) => {
  anchorElement.value = element
}, { immediate: true })

/**
 * Emits the current selection when the card action is available.
 */
function handleSelect() {
  if (!props.canSelectItem) {
    return
  }

  emit('select')
}

/**
 * Handles plain clicks on the card while preserving native modified link clicks.
 *
 * @param event Click event emitted by the card root.
 */
function handleCardClick(event: MouseEvent): void {
  if (!props.canSelectItem) {
    event.preventDefault()
    return
  }

  if (props.href && (event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey)) {
    return
  }

  // Prevent the native link navigation and emit the selection for the SPA router.
  event.preventDefault()
  handleSelect()
}
</script>

<template>
  <article
    ref="cardRef"
    class="media-card"
    :class="[
      `media-card--${thumbnailOrientation}`,
      { 'media-card--preview-open': isPreviewOpen },
    ]"
    tabindex="0"
    @keydown.enter.prevent="handleSelect"
    @focusin="openPreview"
    @focusout="handleFocusOut"
    @mouseenter="openPreview"
    @mouseleave="closePreview"
  >
    <component
      :is="href ? 'a' : 'div'"
      class="media-card__link"
      :href="href ?? undefined"
      :aria-label="displayTitle"
      tabindex="-1"
      @click="handleCardClick"
    >
      <MediaCardPoster
        :display-title="displayTitle"
        :image-poster-url="item.imagePosterUrl"
        :image-landscape-url="item.imageLandscapeUrl"
        :image-url="item.imageUrl"
        :thumbnail-orientation="thumbnailOrientation"
        :thumbnail-image-fit="thumbnailImageFit"
        :image-available="imageAvailable"
        :media-type-label="item.mediaTypeLabel"
        :audio-label="item.audioLabel"
        :duration-label="item.durationLabel"
        :formatted-rating="formattedRating"
        :rating-class="ratingClass"
        :price="item.price"
        :service-title="serviceTitle"
        :service-logo="showServiceLogo ? serviceLogo : null"
        @image-error="$emit('imageError')"
        @service-logo-error="$emit('serviceLogoError')"
      />

      <div v-if="item.title" class="media-card__content">
        <p v-if="item.releaseDateLabel" class="media-card__release-date">
          {{ item.releaseDateLabel }}
        </p>

        <h2 class="media-card__title">
          {{ item.title }}
        </h2>

        <div v-if="item.episodeLabel" class="media-card__meta-row">
          <p v-if="item.episodeLabel" class="media-card__episode">
            {{ item.episodeLabel }}
          </p>
        </div>
      </div>
    </component>

    <MediaCardPreviewPanel
      :item="item"
      :display-title="displayTitle"
      :is-preview-open="isPreviewOpen"
      :thumbnail-orientation="thumbnailOrientation"
      :service-title="serviceTitle"
      :anchor-ref="anchorElement"
      @popup-root-change="(element) => emit('popupRootChange', element)"
    />
  </article>
</template>

<style scoped>
.media-card {
  position: relative;
  min-width: 0;
  overflow: hidden;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  outline: none;
  backdrop-filter: var(--backdrop-filter-soft);
}

.media-card:focus-visible {
  box-shadow:
    0 0 0 2px var(--focus-ring-color),
    0 18px 30px var(--shadow-color);
}

.media-card__link {
  display: block;
  border-radius: inherit;
  color: inherit;
  text-decoration: none;
}

.media-card__content {
  position: relative;
  padding: 4px 10px 4px;
}

.media-card__content::before {
  content: "";
  position: absolute;
  inset: 0;
  z-index: 0;
  border-radius: inherit;
  background: var(--bg-card-background);
  opacity: var(--bg-card-opacity);
  pointer-events: none;
}

.media-card__content > * {
  position: relative;
  z-index: 1;
}

.media-card--landscape .media-card__content {
  padding: 4px 10px 4px;
}

.media-card__release-date {
  display: -webkit-box;
  overflow: hidden;
  margin-bottom: 6px;
  color: var(--text-secondary);
  font-size: 0.78rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  line-height: 1.25;
  text-transform: uppercase;
  text-overflow: ellipsis;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}

.media-card__title {
  display: -webkit-box;
  overflow: hidden;
  margin-bottom: 4px;
  color: var(--text-primary);
  font-size: 1rem;
  font-weight: 700;
  line-height: 1.2;
  text-overflow: ellipsis;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}

.media-card__meta-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin-top: 6px;
}

.media-card__content-rating {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 22px;
  padding: 0 8px;
  border-radius: 999px;
  color: var(--text-primary);
  font-size: 0.78rem;
  font-weight: 800;
  letter-spacing: 0.01em;
  flex: 0 0 auto;
}

.media-card__episode {
  display: -webkit-box;
  overflow: hidden;
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.92rem;
  line-height: 1.28;
  text-overflow: ellipsis;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}

.media-card--landscape .media-card__title {
  font-size: 1.14rem;
  line-height: 1.24;
  -webkit-line-clamp: 2;
}

.media-card--landscape .media-card__release-date {
  font-size: 0.82rem;
}

.media-card--landscape .media-card__content-rating {
  font-size: 0.82rem;
  min-height: 24px;
  padding: 0 10px;
}

.media-card--landscape .media-card__episode {
  font-size: 0.95rem;
  line-height: 1.36;
  -webkit-line-clamp: 3;
}

@media (max-width: 820px) {
  .media-card--landscape .media-card__content {
    padding: 4px 10px 4px;
  }
}
</style>
