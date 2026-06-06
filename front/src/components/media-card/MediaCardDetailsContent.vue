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
}

const props = withDefaults(defineProps<Props>(), {
  variant: 'preview',
  showDurationFact: false,
  serviceTitle: null,
})
const { t } = useI18n()

/**
 * Indicates whether any labeled fact should be rendered.
 */
const showFacts = computed(() =>
  Boolean(
    (props.showDurationFact && props.item.durationLabel) ||
      props.serviceTitle ||
      props.item.releaseDateLabel ||
      props.item.expireLabel,
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

      <p
        v-if="serviceTitle"
        class="media-card-details__fact"
        :class="{ 'media-card-details__fact--list': variant === 'list' }"
      >
        <span class="media-card-details__fact-label">{{ t('media.source') }} :</span>
        {{ ' ' }}{{ serviceTitle }}
      </p>

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

@media (max-width: 720px) {
  .media-card-details__facts--list {
    gap: 6px 14px;
  }
}
</style>
