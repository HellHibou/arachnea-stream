<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, shallowRef, watch } from 'vue'

import VideoPlayer from '@/components/media/VideoPlayer.vue'
import { useI18n } from '@/i18n'
import type { ResolvedBackgroundMediaItem } from '@/composables/background/backgroundMedia'

/**
 * Props accepted by the media layer rendered inside the page background.
 */
interface Props {
  /**
   * Ordered background media items when compatible media is available.
   * @default []
   */
  mediaItems?: ResolvedBackgroundMediaItem[]
  /**
   * CSS modifiers applied to the background image element.
   * @default () => ({})
   */
  backgroundImageClasses?: Record<string, boolean>
}

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
  mediaItems: () => [],
  backgroundImageClasses: () => ({}),
})

/** Delay in milliseconds between automatic background media rotations. */
const AUTO_ROTATION_DELAY_MS = 20000

/** Index of the currently active background media item. */
const activeBackgroundMediaIndex = shallowRef(0)
/** Timer ID for the automatic rotation. */
const rotationTimer = shallowRef<ReturnType<typeof window.setTimeout> | null>(null)
/** Internationalization utilities. */
const { t } = useI18n()

/**
 * Returns the ordered background media items that can be rotated.
 */
const backgroundMediaItems = computed(() =>
  props.mediaItems,
)

/**
 * Returns the active media item currently rendered in the background layer.
 */
const activeBackgroundMediaItem = computed(() =>
  backgroundMediaItems.value[activeBackgroundMediaIndex.value] ?? null,
)

/**
 * Returns the active video source when the background currently renders a video.
 */
const activeBackgroundVideoSource = computed(() =>
  activeBackgroundMediaItem.value?.type === 'video'
    ? activeBackgroundMediaItem.value.source
    : null,
)

/**
 * Returns the active image URL when the background currently renders an image.
 */
const activeBackgroundImageUrl = computed(() =>
  activeBackgroundMediaItem.value?.type === 'image'
    ? activeBackgroundMediaItem.value.src
    : null,
)

/**
 * Returns all image items with their position in the complete background rotation list.
 */
const backgroundImageItems = computed(() =>
  backgroundMediaItems.value.flatMap((item, index) =>
    item.type === 'image'
      ? [{ src: item.src, mediaIndex: index }]
      : [],
  ),
)

/**
 * Per-image Ken Burns pan bounds, expressed as CSS percentages of the frame.
 *
 * The values are derived from the real rendered image overflow under
 * `object-fit: cover`, so the animation translates exactly up to the image
 * borders regardless of the source aspect ratio.
 */
const kenBurnsOffsets = reactive<Record<string, { x: string; y: string }>>({})

/** Natural sizes of loaded images, kept to recompute offsets on viewport resize. */
const kenBurnsNaturalSizes = new Map<string, { width: number; height: number }>()

/**
 * Computes the maximum pan offsets of one image so the animation reaches the
 * exact image edges without ever revealing a gap.
 *
 * @param src Image source URL used as cache key.
 * @param frameWidth Frame width in pixels.
 * @param frameHeight Frame height in pixels.
 */
function computeKenBurnsOffsets(src: string, frameWidth: number, frameHeight: number) {
  const naturalSize = kenBurnsNaturalSizes.get(src)

  if (!naturalSize || frameWidth <= 0 || frameHeight <= 0) {
    return
  }

  const coverScale = Math.max(
    frameWidth / naturalSize.width,
    frameHeight / naturalSize.height,
  )
  const overflowX = Math.max(0, (naturalSize.width * coverScale - frameWidth) / 2)
  const overflowY = Math.max(0, (naturalSize.height * coverScale - frameHeight) / 2)

  kenBurnsOffsets[src] = {
    x: `${((overflowX / frameWidth) * 100).toFixed(4)}%`,
    y: `${((overflowY / frameHeight) * 100).toFixed(4)}%`,
  }
}

/**
 * Remembers the natural size of a loaded image and computes its pan bounds.
 *
 * @param event Load event raised by the background image element.
 * @param src Image source URL used as cache key.
 */
function handleImageLoad(event: Event, src: string) {
  const image = event.target as HTMLImageElement | null

  if (!image?.naturalWidth || !image.naturalHeight) {
    return
  }

  kenBurnsNaturalSizes.set(src, {
    width: image.naturalWidth,
    height: image.naturalHeight,
  })
  computeKenBurnsOffsets(src, window.innerWidth, window.innerHeight)
}

/**
 * Returns the inline CSS variables carrying one image pan bounds.
 *
 * @param src Image source URL.
 * @returns Style object consumed by the Ken Burns keyframes.
 */
function kenBurnsStyle(src: string): Record<string, string> {
  const offsets = kenBurnsOffsets[src]

  if (!offsets) {
    return {}
  }

  return {
    '--ken-burns-max-x': offsets.x,
    '--ken-burns-max-y': offsets.y,
  }
}

/** Recomputes every known image pan bounds after a viewport resize. */
function recomputeAllKenBurnsOffsets() {
  const frameWidth = window.innerWidth
  const frameHeight = window.innerHeight

  for (const src of kenBurnsNaturalSizes.keys()) {
    computeKenBurnsOffsets(src, frameWidth, frameHeight)
  }
}

/**
 * Indicates whether the active native background video should advance on its ended event.
 */
const shouldRotateActiveBackgroundVideoOnEnd = computed(() =>
  backgroundMediaItems.value.length > 1 &&
  activeBackgroundVideoSource.value?.renderer === 'video',
)

/**
 * Indicates whether the active background video should loop instead of rotating to another item.
 */
const shouldLoopActiveBackgroundVideo = computed(() =>
  !shouldRotateActiveBackgroundVideoOnEnd.value,
)

/**
 * Indicates whether the active background item uses the fixed image rotation delay.
 */
const shouldUseTimedAutoRotation = computed(() =>
  backgroundMediaItems.value.length > 1 &&
  !shouldRotateActiveBackgroundVideoOnEnd.value,
)

/**
 * Returns a stable key for the active background video renderer.
 */
const activeBackgroundVideoKey = computed(() => {
  const source = activeBackgroundVideoSource.value

  if (!source) {
    return null
  }

  return `background:${source.renderer}:${source.src}`
})

/**
 * Moves the background to the next available media item.
 */
function showNextBackgroundMedia() {
  const itemCount = backgroundMediaItems.value.length

  if (itemCount <= 1) {
    return
  }

  activeBackgroundMediaIndex.value = (activeBackgroundMediaIndex.value + 1) % itemCount
  restartAutoRotation()
}

/**
 * Moves to the next background item once the active native video reaches its end.
 */
function handleBackgroundVideoPlaybackEnded() {
  if (!shouldRotateActiveBackgroundVideoOnEnd.value) {
    return
  }

  showNextBackgroundMedia()
}

/**
 * Stops the background rotation timer when it is currently active.
 */
function stopAutoRotation() {
  if (rotationTimer.value === null) {
    return
  }

  window.clearTimeout(rotationTimer.value)
  rotationTimer.value = null
}

/**
 * Restarts the background rotation timer when the active item uses the fixed delay.
 */
function restartAutoRotation() {
  stopAutoRotation()

  if (!shouldUseTimedAutoRotation.value) {
    return
  }

  rotationTimer.value = window.setTimeout(() => {
    rotationTimer.value = null
    showNextBackgroundMedia()
  }, AUTO_ROTATION_DELAY_MS)
}

/** Resets rotation state and prunes stale pan data when background items change. */
watch(
  backgroundMediaItems,
  () => {
    activeBackgroundMediaIndex.value = 0
    restartAutoRotation()

    const activeSources = new Set(
      backgroundMediaItems.value
        .filter((item) => item.type === 'image')
        .map((item) => item.src),
    )

    for (const src of [...kenBurnsNaturalSizes.keys()]) {
      if (!activeSources.has(src)) {
        kenBurnsNaturalSizes.delete(src)
        delete kenBurnsOffsets[src]
      }
    }
  },
  { immediate: true },
)

/** Keeps pan bounds accurate when the viewport is resized. */
onMounted(() => {
  window.addEventListener('resize', recomputeAllKenBurnsOffsets)
})

/** Cleans up timers and listeners when the component is unmounted. */
onBeforeUnmount(() => {
  stopAutoRotation()
  window.removeEventListener('resize', recomputeAllKenBurnsOffsets)
})
</script>

<template>
  <VideoPlayer
    v-if="activeBackgroundVideoSource"
    :key="activeBackgroundVideoKey ?? undefined"
    :source="activeBackgroundVideoSource"
    class="background__video"
    :iframe-title="t('media.decorativeBackgroundVideo')"
    autoplay
    :loop="shouldLoopActiveBackgroundVideo"
    muted
    playsinline
    preload="auto"
    allow="autoplay; encrypted-media; picture-in-picture"
    :aria-hidden="true"
    :tab-index="-1"
    @playback-ended="handleBackgroundVideoPlaybackEnded"
  />

  <div v-else-if="activeBackgroundImageUrl" class="background__images">
    <div
      v-for="imageItem in backgroundImageItems"
      :key="`${imageItem.mediaIndex}:${imageItem.src}`"
      class="background__image-frame"
      :class="{ 'background__image-frame--active': imageItem.mediaIndex === activeBackgroundMediaIndex }"
    >
      <img
        class="background__image"
        :class="backgroundImageClasses"
        :style="kenBurnsStyle(imageItem.src)"
        :src="imageItem.src"
        alt=""
        @load="handleImageLoad($event, imageItem.src)"
      >
    </div>
  </div>
</template>

<style scoped>
.background__video {
  position: absolute;
  top: 50%;
  left: 50%;
  width: 100vw;
  min-width: 177.78vh;
  height: 56.25vw;
  min-height: 100vh;
  min-height: 100dvh;
  border: 0;
  transform: translate(-50%, -50%) scale(1.18);
  transform-origin: center;
  filter: saturate(0.82) brightness(0.62);
  will-change: transform;
}

.background__video :deep(.vjs-tech),
.background__video :deep(video) {
  object-fit: cover;
}

.background__images,
.background__image-frame {
  position: absolute;
  inset: 0;
}

.background__image-frame {
  opacity: 0;
  transition: opacity 1400ms ease;
  will-change: opacity;
}

.background__image-frame--active {
  opacity: 1;
}

.background__image {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-position: center;
  transform-origin: center;
  filter: saturate(0.92) brightness(0.88);
  will-change: transform;
}

.background__image--contain {
  object-fit: contain;
}

.background__image--cover {
  object-fit: cover;
}

.background__image--animated {
  transform: scale(1.16) translate3d(
    calc(var(--ken-burns-max-x, 0%) * -1),
    calc(var(--ken-burns-max-y, 0%) * -1),
    0
  );
  animation: background-image-ken-burns 30s cubic-bezier(0.35, 0.04, 0.2, 1) infinite alternate;
}

.background__image--static {
  transform: scale(1);
}

@keyframes background-image-ken-burns {
  0% {
    transform: scale(1.18) translate3d(
      calc(var(--ken-burns-max-x, 0%) * -1),
      calc(var(--ken-burns-max-y, 0%) * -1),
      0
    );
  }

  50% {
    transform: scale(1.3) translate3d(
      calc(var(--ken-burns-max-x, 0%) * -0.25),
      calc(var(--ken-burns-max-y, 0%) * 0.2),
      0
    );
  }

  100% {
    transform: scale(1.42) translate3d(
      var(--ken-burns-max-x, 0%),
      var(--ken-burns-max-y, 0%),
      0
    );
  }
}

@media (prefers-reduced-motion: reduce) {
  .background__image--animated {
    animation: none;
  }
}
</style>
