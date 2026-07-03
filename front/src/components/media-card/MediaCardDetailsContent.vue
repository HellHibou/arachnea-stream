<script setup lang="ts">
import { computed } from 'vue'

import type { MediaItem } from '@/types/media'
import { useI18n } from '@/i18n'

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
   * Indicates whether the duration should be displayed in the facts block.
   * @default false
   */
  showDurationFact?: boolean
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
  showDurationFact: false,
  serviceTitle: null,
  serviceLogo: null,
})
/** Internationalization utilities. */
const { t } = useI18n()

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

/**
 * Indicates whether any labeled fact should be rendered.
 */
const showFacts = computed(() =>
  Boolean(
    (props.showDurationFact && props.item.durationLabel) ||
      props.serviceTitle ||
      props.item.releaseDateLabel ||
      props.item.expireLabel ||
      (props.variant === 'list' && props.item.rating != null),
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
      v-if="item.metaLine"
      class="media-card-details__meta"
      :class="{ 'media-card-details__meta--list': variant === 'list' }"
    >
      {{ item.metaLine }}
    </p>

    <div
      v-if="showFacts"
      class="media-card-details__facts"
      :class="{ 'media-card-details__facts--list': variant === 'list' }"
    >
      <p
        v-if="showDurationFact && item.durationLabel"
        class="media-card-details__fact"
        :class="{ 'media-card-details__fact--list': variant === 'list' }"
      >
        <span class="media-card-details__fact-label">{{ t('media.duration') }} :</span>
        {{ ' ' }}{{ item.durationLabel }}
      </p>

      <div
        v-if="serviceTitle && variant === 'list'"
        class="media-card-details__fact media-card-details__fact--list media-card-details__source"
      >
        <span class="media-card-details__fact-label">{{ t('media.source') }} :</span>
        <span class="media-card-details__source-title">{{ serviceTitle }}</span>
      </div>

      <p
        v-if="item.releaseDateLabel"
        class="media-card-details__fact"
        :class="{ 'media-card-details__fact--list': variant === 'list' }"
      >
        <span class="media-card-details__fact-label">{{ t('media.releaseDate') }} :</span>
        {{ ' ' }}{{ item.releaseDateLabel }}
      </p>

      <p
        v-if="item.expireLabel"
        class="media-card-details__fact"
        :class="{ 'media-card-details__fact--list': variant === 'list' }"
      >
        <span class="media-card-details__fact-label">{{ t('media.availableUntil') }} :</span>
        {{ ' ' }}{{ item.expireLabel }}
      </p>

      <div
        v-if="formattedRating && variant === 'list'"
        class="media-card-details__fact media-card-details__fact--list media-card-details__score"
      >
        <span class="media-card-details__fact-label">{{ t('media.score') }} :</span>
        <span class="media-card-details__score-value" :class="scoreClass">{{ formattedRating }}</span>
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

.media-card-details__facts {
  display: grid;
  gap: 8px;
}

.media-card-details__facts--list {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 18px;
}

.media-card-details__fact,
.media-card-details__episode {
  display: block;
  overflow: visible;
  color: var(--text-secondary);
  font-size: 0.9rem;
  line-height: 1.3;
}

.media-card-details__fact--list {
  margin: 0;
  white-space: nowrap;
}

.media-card-details__fact-label {
  color: var(--text-primary);
  font-weight: 700;
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
  color: var(--text-on-rating-good);
}

.media-card-details__score-value.media-card__rating--average {
  background: var(--bg-rating-average);
  color: var(--text-on-rating-average);
}

.media-card-details__score-value.media-card__rating--low {
  background: var(--bg-rating-low);
  color: var(--text-on-rating-low);
}

@media (max-width: 720px) {
  .media-card-details__facts--list {
    gap: 6px 14px;
  }
}
</style>
