<script setup lang="ts">
import { computed } from 'vue'
import type { ThumbnailImageFit, ThumbnailOrientation } from '@/types/media'

/**
 * Props accepted by the shared media poster component.
 */
interface Props {
  /**
   * Stable title used as the accessible image label.
   */
  displayTitle: string
  /**
   * Portrait poster image displayed inside the card.
   */
  imagePosterUrl: string | null
  /**
   * Landscape poster image displayed inside the card.
   */
  imageLandscapeUrl: string | null
  /**
   * Thumbnail orientation applied to the poster frame.
   */
  thumbnailOrientation: ThumbnailOrientation
  /**
   * Poster fit mode applied to the foreground image.
   */
  thumbnailImageFit: ThumbnailImageFit
  /**
   * Indicates whether the poster image loaded successfully.
   */
  imageAvailable: boolean
  /**
   * Optional badge rendered on the poster.
   */
  mediaTypeLabel?: string | null
  /**
   * Optional audio badge rendered on the poster.
   */
  audioLabel?: string | null
  /**
   * Optional duration badge rendered on the poster.
   */
  durationLabel?: string | null
  /**
   * Optional rating badge rendered on the poster.
   */
  formattedRating?: string | null
  /**
   * CSS class applied to the rating badge.
   */
  ratingClass?: string
  /**
   * Controls whether overlay badges should be displayed.
   * @default true
   */
  showBadges?: boolean
  /**
   * Display title for the backend service attached to this item.
   */
  serviceTitle?: string | null
  /**
   * Logo URL for the backend service attached to this item.
   */
  serviceLogo?: string | null
}

const props = withDefaults(defineProps<Props>(), {
  mediaTypeLabel: null,
  audioLabel: null,
  durationLabel: null,
  formattedRating: null,
  ratingClass: '',
  showBadges: true,
  serviceTitle: null,
  serviceLogo: null,
})

/**
 * Selects the appropriate image source based on orientation preference
 * with fallback to the alternate orientation image if needed.
 */
const imageSource = computed(() => {
  if (props.thumbnailOrientation === 'landscape') {
    return props.imageLandscapeUrl ?? props.imagePosterUrl
  } else {
    return props.imagePosterUrl ?? props.imageLandscapeUrl
  }
})

defineEmits<{
  imageError: []
  serviceLogoError: []
}>()
</script>

<template>
  <div
    class="media-card__poster"
    :class="[
      `media-card__poster--${thumbnailOrientation}`,
      `media-card__poster--fit-${thumbnailImageFit}`,
    ]"
  >
    <!-- Reuse the poster as a decorative blurred backdrop when contain mode would otherwise expose empty edges. -->
    <img
      v-if="thumbnailImageFit === 'contain' && imageSource && imageAvailable"
      class="media-card__image media-card__image--backdrop"
      :src="imageSource"
      loading="lazy"
      alt=""
      aria-hidden="true"
    />

    <img
      v-if="imageSource && imageAvailable"
      class="media-card__image media-card__image--foreground"
      :src="imageSource"
      loading="lazy"
      :alt="displayTitle"
      @error="$emit('imageError')"
    />
    <!-- Keep the visual fallback hidden from assistive tech because the card title already provides the accessible name. -->
    <div v-else class="media-card__fallback" aria-hidden="true">
      <span>{{ displayTitle }}</span>
    </div>

    <div class="media-card__shade" />

    <div
      v-if="serviceLogo || (showBadges && audioLabel)"
      class="media-card__top-right-badges"
      :class="{ 'media-card__top-right-badges--combined': serviceLogo && showBadges && audioLabel }"
    >
      <span v-if="showBadges && audioLabel" class="media-card__audio">
        {{ audioLabel }}
      </span>

      <span
        v-if="serviceLogo"
        class="media-card__preview-source-logo"
        :title="serviceTitle ?? undefined"
      >
        <img
          class="media-card__preview-source-logo-image"
          :src="serviceLogo"
          :alt="serviceTitle ? `${serviceTitle} logo` : ''"
          @error="$emit('serviceLogoError')"
        />
      </span>
    </div>

    <template v-if="showBadges">
      <span v-if="mediaTypeLabel" class="media-card__media-type">
        {{ mediaTypeLabel }}
      </span>

      <span v-if="durationLabel" class="media-card__duration">
        {{ durationLabel }}
      </span>

      <span
        v-if="formattedRating != null"
        class="media-card__rating"
        :class="ratingClass"
      >
        {{ formattedRating }}
      </span>
    </template>
  </div>
</template>

<style scoped>
.media-card__poster {
  position: relative;
  aspect-ratio: 0.72;
  isolation: isolate;
  overflow: hidden;
  background: linear-gradient(160deg, rgba(58, 91, 126, 0.75), rgba(36, 39, 48, 0.95));
}

.media-card__poster--landscape {
  aspect-ratio: 16 / 9;
}

.media-card__image {
  width: 100%;
  height: 100%;
  display: block;
  object-fit: cover;
  object-position: center;
}

.media-card__image--backdrop {
  position: absolute;
  inset: 0;
  z-index: 0;
  filter: var(--filter-media-card-backdrop);
}

.media-card__image--foreground {
  position: relative;
  z-index: 1;
}

.media-card__poster--fit-contain .media-card__image--foreground {
  object-fit: contain;
  background: var(--bg-surface);
}

.media-card__fallback {
  display: grid;
  place-items: center;
  width: 100%;
  height: 100%;
  padding-left: 18px;
  padding-right: 18px;
  background:
    radial-gradient(circle at top, var(--color-primary), transparent 36%),
    linear-gradient(180deg, rgba(24, 28, 38, 0.92), rgba(17, 20, 27, 0.98));
  color: var(--text-primary);
  text-align: center;
  font-size: 1.2rem;
  font-weight: 700;
  text-overflow: ellipsis;
}

.media-card__shade {
  position: absolute;
  inset: auto 0 0;
  z-index: 2;
  height: 38%;
  background: var(--bg-overlay-media-card-shade);
  pointer-events: none;
}

.media-card__preview-source-logo {
  display: inline-flex;
  height: 26px;
  padding: 4px 4px 4px 8px;
  align-items: center;
  justify-content: center;
  flex: 0 0 auto;
  overflow: visible;
  background: var(--bg-surface);
  border-radius:  0 0 0 14px;
  backdrop-filter: var(--backdrop-filter-soft);
}

.media-card__preview-source-logo-image {
  display: block;
  height: 100%;
  max-width: none;
  object-fit: contain;
}

.media-card__top-right-badges {
  position: absolute;
  top: 0;
  right: 0;
  z-index: 3;
  display: flex;
  align-items: flex-start;
  justify-content: flex-end;
  max-width: 100%;
}

.media-card__top-right-badges .media-card__audio,
.media-card__top-right-badges .media-card__preview-source-logo {
  position: static;
}

.media-card__top-right-badges--combined .media-card__preview-source-logo {
  border-radius: 0;
}

.media-card__media-type,
.media-card__audio,
.media-card__duration,
.media-card__rating {
  position: absolute;
  z-index: 3;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 26px;
  padding: 0 10px;
  padding-right: 12px;
  color: var(--text-primary);
  font-size: 0.85rem;
  font-weight: 700;
  letter-spacing: 0.01em;
  backdrop-filter: var(--backdrop-filter-soft);
}

.media-card__media-type {
  top: 0;
  left: 0;
  border-radius: 0 0 14px 0;
  background: var(--bg-surface);
}

.media-card__audio {
  top: 0;
  right: 0;
  border-radius: 0 0 0 14px;
  background: var(--bg-surface);
}

.media-card__duration {
  right: 0;
  bottom: 0;
  border-radius: 14px 0 0 14px;
  background: var(--bg-surface);
}

.media-card__rating {
  bottom: 0;
  left: 0;
  padding-left: 5px;
  border-radius: 0 14px 14px 0;
}

.media-card__rating--good {
  background: var(--bg-rating-good);
}

.media-card__rating--average {
  background: var(--bg-rating-average);
}

.media-card__rating--low {
  background: var(--bg-rating-low);
}
</style>
