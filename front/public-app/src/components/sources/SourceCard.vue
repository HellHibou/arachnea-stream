<script setup lang="ts">
import { ref, watch, type DeepReadonly } from 'vue'
import { RouterLink } from 'vue-router'

import MediaCardPoster from '@/components/media-card/MediaCardPoster.vue'
import { mediaCardPreview } from '@/composables/media-card/mediaCardPreview'
import type { ServiceMetadata } from '@/types/serviceMetadata'

import SourceCardPreviewPanel from './SourceCardPreviewPanel.vue'

/**
 * Props accepted by the source card.
 */
interface Props {
  /** Source metadata rendered by the card. */
  source: DeepReadonly<ServiceMetadata>
}

/** Component props without defaults. */
const props = defineProps<Props>()

/** Whether the source logo loaded successfully. */
const logoAvailable = ref(true)
/** Whether the metadata preview is currently open. */
const isPreviewOpen = ref(false)

/** Preview lifecycle bound to the card root for hover and keyboard focus. */
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
    isPreviewOpen.value = true
  },
  onClose: () => {
    isPreviewOpen.value = false
  },
})

watch(() => props.source.logo, () => {
  logoAvailable.value = true
})
</script>

<template>
  <article
    ref="cardRef"
    class="source-card"
    @focusin="openPreview"
    @focusout="handleFocusOut"
    @mouseenter="openPreview"
    @mouseleave="closePreview"
  >
    <RouterLink
      class="source-card__link"
      :to="{ name: 'source-home', params: { sourceId: source.id } }"
      :aria-label="source.title"
    >
      <MediaCardPoster
        v-if="source.logo && logoAvailable"
        class="source-card__poster"
        :display-title="source.title"
        :image-poster-url="source.logo"
        :image-landscape-url="source.logo"
        :image-url="source.logo"
        thumbnail-orientation="landscape"
        thumbnail-image-fit="contain"
        :image-available="true"
        :show-badges="false"
        @image-error="logoAvailable = false"
      />
      <span v-else class="source-card__fallback">
        <v-icon icon="mdi-television" size="40" aria-hidden="true" />
      </span>
      <span class="source-card__name">{{ source.title }}</span>
    </RouterLink>

    <SourceCardPreviewPanel
      :source="source"
      :is-preview-open="isPreviewOpen"
      :anchor-ref="cardRef"
    />
  </article>
</template>

<style scoped>
.source-card {
  min-width: 0;
  overflow: hidden;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  box-shadow: var(--inset-light);
  transition:
    border-color var(--duration-fast) ease,
    background-color var(--duration-fast) ease,
    transform var(--duration-fast) ease;
}

.source-card:hover,
.source-card:focus-within {
  border-color: var(--color-primary);
  background: var(--bg-surface-strong);
  transform: translateY(-2px);
}

.source-card__link {
  display: flex;
  flex-direction: column;
  border-radius: inherit;
  color: var(--text-primary);
  text-decoration: none;
}

.source-card__link:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: -3px;
}

.source-card__poster {
  display: block;
  width: 100%;
}

.source-card__fallback {
  display: grid;
  width: 100%;
  aspect-ratio: 16 / 9;
  place-items: center;
  background:
    radial-gradient(circle at top, var(--color-primary), transparent 36%),
    linear-gradient(180deg, rgba(24, 28, 38, 0.92), rgba(17, 20, 27, 0.98));
  color: var(--text-secondary);
}

.source-card__name {
  display: -webkit-box;
  overflow: hidden;
  padding: 10px 12px 12px;
  color: var(--text-primary);
  font-size: 1rem;
  font-weight: 700;
  line-height: 1.25;
  text-overflow: ellipsis;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}

@media (prefers-reduced-motion: reduce) {
  .source-card {
    transition: none;
  }

  .source-card:hover,
  .source-card:focus-within {
    transform: none;
  }
}
</style>
