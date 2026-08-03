<script setup lang="ts">
import type { MediaItem, ThumbnailImageFit, ThumbnailOrientation } from '@/types/media'

import MediaCardActionButton from './MediaCardActionButton.vue'
import MediaCardListContent from './MediaCardListContent.vue'
import MediaCardPoster from './MediaCardPoster.vue'

/**
 * Props accepted by the list media card layout.
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
   * Formatted rating displayed in the list badge.
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
   * Indicates whether the list thumbnail column should be displayed.
   */
  showListPoster: boolean
  /**
   * Indicates whether the poster image loaded successfully.
   */
  imageAvailable: boolean
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
defineProps<Props>()

defineEmits<{
  /** Emitted when the poster image fails to load. */
  imageError: []
  /** Emitted when the service logo fails to load. */
  serviceLogoError: []
  /** Emitted when the card is selected. */
  select: []
}>()
</script>

<template>
  <article
    class="media-card"
    :class="[
      'media-card--list',
      `media-card--${thumbnailOrientation}`,
      { 'media-card--list-no-poster': !showListPoster },
    ]"
  >
    <div v-if="showListPoster" class="media-card__list-poster">
      <MediaCardPoster
        :display-title="displayTitle"
        :image-poster-url="item.imagePosterUrl"
        :image-landscape-url="item.imageLandscapeUrl"
        :thumbnail-orientation="thumbnailOrientation"
        :thumbnail-image-fit="thumbnailImageFit"
        :image-available="imageAvailable"
        :show-badges="false"
        :service-title="serviceTitle"
        :service-logo="showServiceLogo ? serviceLogo : null"
        @image-error="$emit('imageError')"
        @service-logo-error="$emit('serviceLogoError')"
      />
    </div>

    <MediaCardListContent
      :item="item"
      :display-title="displayTitle"
      :formatted-rating="formattedRating"
      :rating-class="ratingClass"
      :service-title="serviceTitle"
      :service-logo="serviceLogo"
    />

    <div class="media-card__list-action">
      <MediaCardActionButton
        :can-select-item="canSelectItem"
        :href="href"
        is-list
        @select="$emit('select')"
      />
    </div>
  </article>
</template>

<style scoped>
.media-card {
  position: relative;
  min-width: 0;
  overflow: hidden;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  box-shadow: var(--shadow-card);
}

.media-card--list {
  display: grid;
  grid-template-columns: minmax(128px, 176px) minmax(0, 1fr) auto;
  gap: 14px;
  align-items: start;
  padding: 12px;
}

.media-card--list.media-card--landscape {
  grid-template-columns: minmax(180px, 240px) minmax(0, 1fr) auto;
}

.media-card--list.media-card--list-no-poster,
.media-card--list.media-card--landscape.media-card--list-no-poster {
  grid-template-columns: minmax(0, 1fr) auto;
}

.media-card__list-poster {
  min-width: 0;
  align-self: start;
}

.media-card__list-poster :deep(.media-card__poster) {
  border-radius: var(--radius);
  box-shadow: 0 12px 24px var(--shadow-color);
}

.media-card__list-action {
  display: flex;
  align-self: start;
  align-items: center;
  justify-content: flex-end;
  justify-self: end;
}

@media (max-width: 720px) {
  .media-card--list,
  .media-card--list.media-card--landscape {
    grid-template-columns: 1fr;
    gap: 10px;
  }

  .media-card__list-action {
    justify-content: flex-start;
    justify-self: stretch;
  }
}
</style>
