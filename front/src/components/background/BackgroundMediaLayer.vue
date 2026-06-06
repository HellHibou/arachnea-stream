<script setup lang="ts">
import { computed, onBeforeUnmount, shallowRef, watch } from 'vue'

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

const props = withDefaults(defineProps<Props>(), {
  mediaItems: () => [],
  backgroundImageClasses: () => ({}),
})

const AUTO_ROTATION_DELAY_MS = 20000

const activeBackgroundMediaIndex = shallowRef(0)
const rotationTimer = shallowRef<ReturnType<typeof window.setTimeout> | null>(null)
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

watch(
  backgroundMediaItems,
  () => {
    activeBackgroundMediaIndex.value = 0
    restartAutoRotation()
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  stopAutoRotation()
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
        :src="imageItem.src"
        alt=""
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
  transform: scale(1.16);
  animation: background-image-ken-burns 30s cubic-bezier(0.35, 0.04, 0.2, 1) infinite alternate;
}

.background__image--static {
  transform: scale(1);
}

@keyframes background-image-ken-burns {
  0% {
    transform: scale(1.18) translate3d(-4.4%, -3.1%, 0);
  }

  45% {
    transform: scale(1.3) translate3d(-1.2%, 0.2%, 0);
  }

  100% {
    transform: scale(1.42) translate3d(4.8%, 3.6%, 0);
  }
}

@media (prefers-reduced-motion: reduce) {
  .background__image--animated {
    animation: none;
  }
}
</style>
