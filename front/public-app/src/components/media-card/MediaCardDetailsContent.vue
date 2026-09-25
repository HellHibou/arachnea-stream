<script setup lang="ts">
import { computed } from 'vue'

import type { MediaItem } from '@/types/media'
import { useI18n } from '@/i18n'
import { formatPriceAccess } from '@/services/priceAccess'

/**
 * Props accepted by the shared media card details content block.
 */
interface Props {
  /**
   * Media item rendered by the card.
   */
  item: MediaItem
  /**
   * Layout variant rendered by the details block.
   * @default 'preview'
   */
  variant?: 'preview' | 'list'
  /**
   * Display title for the backend service attached to the previewed item.
   */
  serviceTitle?: string | null
  /**
   * Logo URL for the backend service attached to the previewed item.
   */
  serviceLogo?: string | null
}

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
  variant: 'preview',
  serviceTitle: null,
  serviceLogo: null,
})
/** Internationalization utilities. */
const { resolvedLanguage, t } = useI18n()

/** Display label for the item paid-access condition, when it needs one. */
const priceLabel = computed(() =>
  formatPriceAccess(props.item.price, resolvedLanguage.value, t),
)

/** Indicates whether the paid-access fact uses the premium crown treatment. */
const showPremiumCrown = computed(() =>
  props.item.price === 'premium' && Boolean(priceLabel.value),
)

/**
 * CSS class for score color based on rating value (matches other components).
 */
const scoreClass = computed(() => {
  if (props.item.rating == null) {
    return ''
  }
  if (props.item.rating >= 4) {
    return 'media-card__rating--good'
  }
  if (props.item.rating >= 3) {
    return 'media-card__rating--average'
  }
  return 'media-card__rating--low'
})

/**
 * Formatted rating value for display (raw rating with 1 decimal, comma as decimal separator).
 */
const formattedRating = computed(() => {
  if (props.item.rating == null) {
    return null
  }
  return props.item.rating.toFixed(1).replace('.', ',')
})

/** Language label rendered as a labeled fact instead of the compact meta line. */
const languageFact = computed(() => props.item.audioLabel)

/** Media type label rendered as a labeled fact instead of the compact meta line. */
const mediaTypeFact = computed(() => props.item.mediaTypeLabel)

/** Genre labels joined for the labeled fact rendered instead of the compact meta line. */
const genreFact = computed(() =>
  props.item.themeLabels.length > 0 ? props.item.themeLabels.join(', ') : null,
)

/**
 * Indicates whether the labeled catalog facts replace the compact meta line,
 * which would otherwise repeat the same information.
 */
const replacesMetaLine = computed(() =>
  Boolean(languageFact.value || mediaTypeFact.value || genreFact.value),
)

/**
 * Indicates whether any labeled fact should be rendered.
 */
const showFacts = computed(() =>
  Boolean(
    languageFact.value ||
      mediaTypeFact.value ||
      genreFact.value ||
      props.item.durationLabel ||
      props.serviceTitle ||
      props.item.releaseDateLabel ||
      props.item.expireLabel ||
      priceLabel.value ||
      props.item.rating != null,
  ),
)

/**
 * Indicates whether the details block should be rendered at all.
 */
const hasDetails = computed(() =>
  Boolean(
    props.item.metaLine ||
      showFacts.value ||
      props.item.episodeLabel ||
      props.item.overview,
  ),
)
</script>

<template>
  <div
    v-if="hasDetails"
    class="media-card-details"
    :class="`media-card-details--${variant}`"
  >
    <p
      v-if="item.metaLine && !replacesMetaLine"
      class="media-card-details__meta"
      :class="{ 'media-card-details__meta--list': variant === 'list' }"
    >
      {{ item.metaLine }}
    </p>

    <div v-if="showFacts" class="media-card-details__facts">
      <p v-if="mediaTypeFact" class="media-card-details__fact">
        <span class="media-card-details__fact-label">{{ t('media.type') }} :</span>
        <span class="media-card-details__fact-value">{{ mediaTypeFact }}</span>
      </p>

      <p v-if="genreFact" class="media-card-details__fact">
        <span class="media-card-details__fact-label">{{ t('media.theme') }} :</span>
        <span class="media-card-details__fact-value">{{ genreFact }}</span>
      </p>

      <p v-if="item.durationLabel" class="media-card-details__fact">
        <span class="media-card-details__fact-label">{{ t('media.duration') }} :</span>
        <span class="media-card-details__fact-value">{{ item.durationLabel }}</span>
      </p>

      <p v-if="languageFact" class="media-card-details__fact">
        <span class="media-card-details__fact-label">{{ t('entry.language') }} :</span>
        <span class="media-card-details__fact-value">{{ languageFact }}</span>
      </p>

      <p v-if="item.releaseDateLabel" class="media-card-details__fact">
        <span class="media-card-details__fact-label">{{ t('media.releaseDate') }} :</span>
        <span class="media-card-details__fact-value">{{ item.releaseDateLabel }}</span>
      </p>

      <p v-if="item.expireLabel" class="media-card-details__fact">
        <span class="media-card-details__fact-label">{{ t('media.availableUntil') }} :</span>
        <span class="media-card-details__fact-value">{{ item.expireLabel }}</span>
      </p>

      <p v-if="priceLabel" class="media-card-details__fact">
        <span
          v-if="showPremiumCrown"
          class="media-card-details__premium-crown"
          aria-hidden="true"
        >
          <v-icon icon="mdi-crown" size="14" />
        </span>
        {{ priceLabel }}
      </p>

      <div v-if="formattedRating" class="media-card-details__fact media-card-details__score">
        <span class="media-card-details__fact-label">{{ t('media.score') }} :</span>
        <span class="media-card-details__score-value" :class="scoreClass">{{ formattedRating }}</span>
      </div>

      <div v-if="serviceTitle" class="media-card-details__fact media-card-details__source">
        <span class="media-card-details__fact-label">{{ t('media.source') }} :</span>
        <span class="media-card-details__source-title">{{ serviceTitle }}</span>
      </div>
    </div>

    <p v-if="item.episodeLabel" class="media-card-details__episode">
      {{ item.episodeLabel }}
    </p>

    <p
      v-if="item.overview"
      class="media-card-details__overview"
      :class="{ 'media-card-details__overview--list': variant === 'list' }"
    >
      {{ item.overview }}
    </p>
  </div>
</template>

<style scoped>
.media-card-details {
  display: grid;
  gap: 8px;
}

.media-card-details__meta {
  color: var(--text-secondary);
  font-size: 0.84rem;
  font-weight: 700;
  letter-spacing: 0.01em;
}

.media-card-details__meta--list,
.media-card-details__overview--list {
  margin: 0;
}

/* Facts flow as inline label+value groups separated by a dot; each group
   wraps to the next line as a whole (its separator stays with the preceding
   group) without being cut. The 6px rhythm is uniform: label to value, value
   to separator, and separator to the next group. */
.media-card-details__facts {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px 6px;
}

.media-card-details__fact {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  overflow: visible;
  color: var(--text-secondary);
  font-size: 0.9rem;
  line-height: 1.3;
  white-space: nowrap;
}

.media-card-details__fact:not(:last-child)::after {
  content: "•";
  color: var(--text-disabled);
}

.media-card-details__fact-label {
  color: var(--text-primary);
  font-weight: 700;
}

/* Premium crown shown before the paid-access fact, matching the card badge. */
.media-card-details__premium-crown {
  display: inline-flex;
  align-items: center;
  color: var(--color-premium-crown);
}

.media-card-details__episode {
  display: block;
  overflow: visible;
  color: var(--text-secondary);
  font-size: 0.9rem;
  line-height: 1.3;
}

.media-card-details__overview {
  display: block;
  overflow: visible;
  color: var(--text-secondary);
  font-size: 0.88rem;
  line-height: 1.42;
  text-align: left;
  text-wrap: pretty;
  hyphens: auto;
}

.media-card-details__source {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text-secondary);
  font-size: 0.9rem;
  line-height: 1.3;
}

.media-card-details__source-title {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text-secondary);
}

.media-card-details__source-logo {
  width: 20px;
  height: 20px;
  border-radius: 4px;
  object-fit: contain;
  background: var(--bg-surface);
  flex-shrink: 0;
}

.media-card-details__score {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.9rem;
  line-height: 1.3;
}

.media-card-details__score-value {
  display: inline-flex;
  align-items: center;
  min-height: 28px;
  padding: 0 12px;
  border-radius: 999px;
  background: var(--bg-surface);
  color: var(--text-primary);
  font-size: 0.82rem;
  font-weight: 700;
  letter-spacing: 0.01em;
}

.media-card-details__score-value.media-card__rating--good {
  background: var(--bg-rating-good);
  color: var(--text-primary);
}

.media-card-details__score-value.media-card__rating--average {
  background: var(--bg-rating-average);
  color: var(--text-primary);
}

.media-card-details__score-value.media-card__rating--low {
  background: var(--bg-rating-low);
  color: var(--text-primary);
}
</style>
