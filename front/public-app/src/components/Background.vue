<script setup lang="ts">
import { toRef } from 'vue'

import type { BackgroundMediaCandidate, ThumbnailImageFit } from '@/types/media'
import type { ResolvedPlayerMediaSource } from '@/services/players'

import BackgroundAurora from './background/BackgroundAurora.vue'
import BackgroundMediaLayer from './background/BackgroundMediaLayer.vue'
import { backgroundMedia } from '@/composables/background/backgroundMedia'

/**
 * Props accepted by the full-page background component.
 */
interface Props {
  /**
   * Optional decorative background video rendered behind the whole page.
   * @default null
   */
  videoUrl?: string | null
  /**
   * Optional resolved decorative background video preserving manifest and DRM metadata.
   * @default null
   */
  videoSource?: ResolvedPlayerMediaSource | null
  /**
   * Optional image rendered behind the whole page.
   * @default null
   */
  imageUrl?: string | null
  /**
   * Optional portrait-oriented image rendered behind the whole page.
   * Selected when viewport height > width.
   * @default null
   */
  imagePortraitUrl?: string | null
  /**
   * Optional landscape-oriented image rendered behind the whole page.
   * Selected when viewport width > height.
   * @default null
   */
  imageLandscapeUrl?: string | null
  /**
   * Optional images rendered behind the whole page.
   * @default []
   */
  imageUrls?: string[]
  /**
   * Optional ordered media candidates rendered behind the whole page.
   * @default []
   */
  mediaItems?: BackgroundMediaCandidate[]
  /**
   * Enables a decorative motion effect on the background media.
   * @default false
   */
  isAnimated?: boolean
  /**
   * Controls whether the background image should be fully visible or cropped.
   * @default 'contain'
   */
  imageFit?: ThumbnailImageFit
}

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
  videoUrl: null,
  videoSource: null,
  imageUrl: null,
  imagePortraitUrl: null,
  imageLandscapeUrl: null,
  imageUrls: () => [],
  mediaItems: () => [],
  isAnimated: false,
  imageFit: 'contain',
})

/** Processed background media items from the composable. */
const {
  /** List of background media items to render. */
  backgroundMediaItems,
  /** Whether the fallback background should be displayed. */
  isFallbackBackground,
  /** CSS classes to apply to the background image element. */
  backgroundImageClasses,
} =
  backgroundMedia({
    videoUrl: toRef(props, 'videoUrl'),
    videoSource: toRef(props, 'videoSource'),
    imageUrl: toRef(props, 'imageUrl'),
    imagePortraitUrl: toRef(props, 'imagePortraitUrl'),
    imageLandscapeUrl: toRef(props, 'imageLandscapeUrl'),
    imageUrls: toRef(props, 'imageUrls'),
    mediaItems: toRef(props, 'mediaItems'),
    isAnimated: toRef(props, 'isAnimated'),
    imageFit: toRef(props, 'imageFit'),
  })
</script>

<template>
  <div
    class="background"
    aria-hidden="true"
  >
    <BackgroundAurora
      v-if="isFallbackBackground"
      :is-animated="props.isAnimated"
    />

    <BackgroundMediaLayer
      :media-items="backgroundMediaItems"
      :background-image-classes="backgroundImageClasses"
    />

    <div
      class="background__veil"
      :class="{ 'background__veil--fallback': isFallbackBackground }"
    />
  </div>
</template>

<style scoped>
.background {
  position: fixed;
  inset: 0;
  z-index: 0;
  overflow: hidden;
  pointer-events: none;
  background: linear-gradient(180deg, #11151c 0%, #0c1117 44%, #080b10 100%);
}

.background__veil {
  position: absolute;
  inset: 0;
  background:
    radial-gradient(circle at top right, rgba(255, 88, 88, 0.08), transparent 30%),
    radial-gradient(circle at left center, rgba(41, 96, 173, 0.14), transparent 38%),
    linear-gradient(
      180deg,
      rgba(9, 12, 18, 0.16) 0%,
      rgba(12, 15, 22, 0.32) 24%,
      rgba(12, 15, 22, 0.56) 58%,
      rgba(12, 15, 22, 0.84) 100%
    );
}

.background__veil--fallback {
  background:
    radial-gradient(ellipse at 50% 60%, rgba(255, 255, 255, 0.05) 0%, transparent 60%),
    linear-gradient(
      180deg,
      rgba(8, 11, 16, 0.06) 0%,
      rgba(8, 11, 16, 0.14) 34%,
      rgba(7, 10, 15, 0.36) 66%,
      rgba(5, 7, 11, 0.72) 100%
    );
}
</style>
