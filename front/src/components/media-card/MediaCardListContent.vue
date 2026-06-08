<script setup lang="ts">
import type { MediaItem } from '@/types/media'

import MediaCardDetailsContent from './MediaCardDetailsContent.vue'

/**
 * Props accepted by the list media card content block.
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
   * Formatted rating displayed in the list badge.
   */
  formattedRating: string | null
  /**
   * CSS class applied to the rating badge.
   */
  ratingClass: string
  /**
   * Display title for the backend service attached to this item.
   */
  serviceTitle: string | null
  /**
   * Logo URL for the backend service attached to this item.
   */
  serviceLogo: string | null
}

const props = withDefaults(defineProps<Props>(), {
  serviceTitle: null,
  serviceLogo: null,
})
</script>

<template>
  <div class="media-card__list-copy">
    <div class="media-card__list-body">
      <div v-if="item.audioLabel" class="media-card__list-badges">
        <span class="media-card__list-badge">{{ item.audioLabel }}</span>
      </div>

      <h2 class="media-card__preview-title media-card__preview-title--list">
        {{ displayTitle }}
      </h2>

      <p v-if="item.alternativeTitleLabel" class="media-card__preview-subtitle">
        <em class="media-card__alternative-title-value">{{ item.alternativeTitleLabel }}</em>
      </p>

      <MediaCardDetailsContent
        :item="item"
        variant="list"
        :show-duration-fact="true"
        :service-title="serviceTitle"
        :service-logo="serviceLogo"
      />
    </div>
  </div>
</template>

<style scoped>
.media-card__list-copy {
  display: flex;
  align-self: stretch;
  min-width: 0;
  flex-direction: column;
  gap: 16px;
}

.media-card__list-body {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 8px;
}

.media-card__list-badges {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.media-card__list-badge {
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

.media-card__list-badge--rating {
  color: var(--text-primary);
}

.media-card__preview-title {
  display: block;
  overflow: visible;
  color: var(--text-primary);
  font-size: 1.1rem;
  font-weight: 800;
  line-height: 1.2;
}

.media-card__preview-title--list {
  font-size: clamp(1.08rem, 1rem + 0.38vw, 1.34rem);
}

.media-card__alternative-title-value {
  color: var(--text-primary);
  font-size: 1rem;
  line-height: 1.4;
  text-shadow: var(--text-shadow-primary);
  display: block;
  margin-top: 4px;
  font-style: italic;
}

@media (max-width: 720px) {
  .media-card__list-copy {
    gap: 14px;
  }
}
</style>
