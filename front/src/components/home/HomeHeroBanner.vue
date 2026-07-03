<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, shallowRef, useTemplateRef, watch } from 'vue'

import {
  resolveBackgroundMediaSource,
  type ResolvedPlayerMediaSource,
} from '@/services/players'
import VideoPlayer from '@/components/media/VideoPlayer.vue'
import type { VideoJsMediaDimensions } from '@/composables/video/useVideoJsMediaRenderer'
import { useI18n } from '@/i18n'
import type { HomeBanner } from '@/types/home'
import type { MediaSelectionTarget } from '@/types/media'

/**
 * Props accepted by the featured banner carousel.
 */
interface Props {
  /**
   * Banners exposed by the backend catalog payload.
   */
  banners: HomeBanner[]
}

/** Component props without defaults. */
const props = defineProps<Props>()

const emit = defineEmits<{
  /** Emitted when a media item is selected from a banner. */
  'select-item': [target: MediaSelectionTarget]
}>()

/** Delay in milliseconds between automatic banner rotations. */
const AUTO_ROTATION_DELAY_MS = 20000
/** Default aspect ratio for banner videos. */
const DEFAULT_BANNER_VIDEO_ASPECT_RATIO = 16 / 9

/** Index of the currently active banner. */
const activeIndex = shallowRef(0)
/** Timer ID for the automatic rotation. */
const autoRotationTimer = shallowRef<ReturnType<typeof window.setTimeout> | null>(null)
/** Template reference to the banner element. */
const bannerElement = useTemplateRef<HTMLElement>('bannerElement')
/** Resize observer for the banner element. */
const bannerResizeObserver = shallowRef<ResizeObserver | null>(null)
/** Current size of the banner element. */
const bannerSize = shallowRef({ width: 0, height: 0 })
/** Current video aspect ratio for the active banner. */
const bannerVideoAspectRatio = shallowRef(DEFAULT_BANNER_VIDEO_ASPECT_RATIO)
/** Whether the banner video is muted. */
const isBannerVideoMuted = shallowRef(true)
/** Internationalization utilities. */
const { t } = useI18n()

/**
 * Returns the banner currently visible in the hero surface.
 */
const activeBanner = computed(() => props.banners[activeIndex.value] ?? null)

/**
 * Returns the resolved video source displayed behind the active banner.
 */
const activeBannerVideoSource = computed<ResolvedPlayerMediaSource | null>(() =>
  resolveBackgroundMediaSource(activeBanner.value?.videoUrl ?? null),
)

/**
 * Returns the banner video source configured with the current banner-only sound preference.
 */
const activeBannerPlayableVideoSource = computed<ResolvedPlayerMediaSource | null>(() => {
  const source = activeBannerVideoSource.value

  if (!source || source.renderer !== 'iframe') {
    return source
  }

  try {
    const url = new URL(source.src)
    url.searchParams.set('mute', isBannerVideoMuted.value ? '1' : '0')

    return {
      ...source,
      src: url.toString(),
    }
  } catch {
    return source
  }
})

/**
 * Returns a stable key that forces the active banner media element to remount when it changes.
 */
const activeBannerMediaKey = computed(() => {
  const banner = activeBanner.value
  const source = activeBannerPlayableVideoSource.value

  if (!banner || !source) {
    return null
  }

  return `${banner.id}:${source.renderer}:${source.src}`
})

/**
 * Returns the banner video dimensions that keep the loaded source fully visible.
 */
const activeBannerVideoStyle = computed(() => {
  const bannerWidth = bannerSize.value.width
  const bannerHeight = bannerSize.value.height
  const aspectRatio = bannerVideoAspectRatio.value

  if (
    bannerWidth <= 0 ||
    bannerHeight <= 0 ||
    !Number.isFinite(aspectRatio) ||
    aspectRatio <= 0
  ) {
    return {
      width: '100%',
      height: '100%',
      aspectRatio: String(DEFAULT_BANNER_VIDEO_ASPECT_RATIO),
    }
  }

  const heightLimitedWidth = bannerHeight * aspectRatio

  if (heightLimitedWidth <= bannerWidth) {
    return {
      width: `${heightLimitedWidth}px`,
      height: `${bannerHeight}px`,
      aspectRatio: String(aspectRatio),
    }
  }

  return {
    width: `${bannerWidth}px`,
    height: `${bannerWidth / aspectRatio}px`,
    aspectRatio: String(aspectRatio),
  }
})

/**
 * Returns the icon that communicates the current banner video audio state.
 */
const bannerVideoSoundIcon = computed(() =>
  isBannerVideoMuted.value ? 'mdi-volume-off' : 'mdi-volume-high',
)

/**
 * Returns the accessible label for the banner-only sound toggle.
 */
const bannerVideoSoundLabel = computed(() =>
  isBannerVideoMuted.value
    ? t('catalog.enableBannerSound')
    : t('catalog.disableBannerSound'),
)

/**
 * Indicates whether the active banner should wait for its native video to end before rotating.
 */
const shouldRotateActiveBannerOnVideoEnd = computed(() =>
  props.banners.length > 1 &&
  activeBannerPlayableVideoSource.value?.renderer === 'video',
)

/**
 * Indicates whether the active banner should keep the fixed image rotation delay.
 */
const shouldUseTimedAutoRotation = computed(() =>
  props.banners.length > 1 && !shouldRotateActiveBannerOnVideoEnd.value,
)

/**
 * Indicates whether the active banner video should loop instead of rotating to another banner.
 */
const shouldLoopActiveBannerVideo = computed(() =>
  !shouldRotateActiveBannerOnVideoEnd.value,
)

/**
 * Returns the target that should open the entry details screen for the active banner.
 */
const activeBannerTarget = computed<MediaSelectionTarget | null>(() => {
  const banner = activeBanner.value
  const entryUrl = banner?.entryUrl ?? banner?.webUrl ?? null

  if (!banner?.source || !entryUrl) {
    return null
  }

  return {
    source: banner.source,
    entryUrl,
    webUrl: banner.webUrl ?? entryUrl,
  }
})

watch(
  () => props.banners.length,
  (length) => {
    if (length === 0) {
      activeIndex.value = 0
      return
    }

    activeIndex.value = Math.min(activeIndex.value, length - 1)
  },
  { immediate: true },
)

watch(
  () => props.banners,
  () => {
    restartAutoRotation()
  },
  { immediate: true },
)

watch(activeBannerMediaKey, () => {
  bannerVideoAspectRatio.value = DEFAULT_BANNER_VIDEO_ASPECT_RATIO
})

/**
 * Stores the current rendered banner size used to contain the decorative video.
 *
 * @param entry Resize observer entry received for the banner root.
 */
function updateBannerSize(entry?: ResizeObserverEntry) {
  const rect = entry?.contentRect ?? bannerElement.value?.getBoundingClientRect()

  if (!rect) {
    return
  }

  bannerSize.value = {
    width: rect.width,
    height: rect.height,
  }
}

/**
 * Captures the intrinsic video ratio once the decorative banner video metadata is loaded.
 *
 * @param dimensions Natural video dimensions emitted by the shared video renderer.
 */
function handleBannerVideoMetadataLoaded(dimensions: VideoJsMediaDimensions) {
  const aspectRatio = Number.isFinite(dimensions.aspectRatio) && dimensions.aspectRatio > 0
    ? dimensions.aspectRatio
    : dimensions.width / dimensions.height

  if (!Number.isFinite(aspectRatio) || aspectRatio <= 0) {
    return
  }

  bannerVideoAspectRatio.value = aspectRatio
}

/**
 * Toggles audio for the decorative video rendered only inside the banner.
 */
function toggleBannerVideoSound() {
  isBannerVideoMuted.value = !isBannerVideoMuted.value
}

/**
 * Moves to the next banner once the active native banner video reaches its end.
 */
function handleBannerVideoPlaybackEnded() {
  if (!shouldRotateActiveBannerOnVideoEnd.value) {
    return
  }

  showNextBanner()
}

/**
 * Moves the carousel to the previous banner.
 */
function showPreviousBanner() {
  if (props.banners.length <= 1) {
    return
  }

  activeIndex.value = (activeIndex.value - 1 + props.banners.length) % props.banners.length
  restartAutoRotation()
}

/**
 * Moves the carousel to the next banner.
 */
function showNextBanner() {
  if (props.banners.length <= 1) {
    return
  }

  activeIndex.value = (activeIndex.value + 1) % props.banners.length
  restartAutoRotation()
}

/**
 * Activates one specific banner from the pagination controls.
 *
 * @param index Banner index selected by the user.
 */
function showBanner(index: number) {
  activeIndex.value = index
  restartAutoRotation()
}

/**
 * Emits the active banner target so the parent can open the entry details screen.
 */
function handleSelectBanner() {
  if (!activeBannerTarget.value) {
    return
  }

  emit('select-item', activeBannerTarget.value)
}

/**
 * Stops the automatic banner rotation timer when it is currently active.
 */
function stopAutoRotation() {
  if (autoRotationTimer.value === null) {
    return
  }

  window.clearTimeout(autoRotationTimer.value)
  autoRotationTimer.value = null
}

/**
 * Restarts the automatic banner rotation when the active banner uses the fixed image delay.
 */
function restartAutoRotation() {
  stopAutoRotation()

  if (!shouldUseTimedAutoRotation.value) {
    return
  }

  autoRotationTimer.value = window.setTimeout(() => {
    autoRotationTimer.value = null
    showNextBanner()
  }, AUTO_ROTATION_DELAY_MS)
}

/** Sets up resize observer and initial size measurement when component mounts. */
onMounted(() => {
  const element = bannerElement.value

  updateBannerSize()

  if (!element || typeof ResizeObserver === 'undefined') {
    return
  }

  const resizeObserver = new ResizeObserver((entries) => {
    updateBannerSize(entries[0])
  })

  resizeObserver.observe(element)
  bannerResizeObserver.value = resizeObserver
})

/** Cleans up rotation timer and resize observer when component unmounts. */
onBeforeUnmount(() => {
  stopAutoRotation()
  bannerResizeObserver.value?.disconnect()
  bannerResizeObserver.value = null
})
</script>

<template>
  <section
    v-if="activeBanner"
    ref="bannerElement"
    class="home-hero-banner"
    :aria-label="activeBanner.title ?? t('catalog.featuredContent')"
  >
      <img
      v-if="activeBanner.imageUrl && !activeBannerVideoSource"
        class="home-hero-banner__image"
        :src="activeBanner.imageUrl"
        alt=""
        aria-hidden="true"
      >

    <VideoPlayer
      v-if="activeBannerPlayableVideoSource"
      :key="activeBannerMediaKey ?? undefined"
      :source="activeBannerPlayableVideoSource"
      :class="[
        'home-hero-banner__video',
        activeBannerPlayableVideoSource.renderer === 'iframe' ? 'home-hero-banner__video--frame' : null,
      ]"
      :style="activeBannerVideoStyle"
        :iframe-title="t('media.decorativeBannerVideo')"
        :poster="activeBanner.imageUrl ?? undefined"
        autoplay
        :muted="isBannerVideoMuted"
        :loop="shouldLoopActiveBannerVideo"
        playsinline
        preload="auto"
        allow="autoplay; encrypted-media; picture-in-picture"
        :aria-hidden="true"
        :tab-index="-1"
        @playback-ended="handleBannerVideoPlaybackEnded"
        @video-metadata-loaded="handleBannerVideoMetadataLoaded"
      />

    <button
      v-if="activeBannerPlayableVideoSource"
      class="home-hero-banner__sound-toggle"
      type="button"
      :aria-label="bannerVideoSoundLabel"
      :aria-pressed="!isBannerVideoMuted"
      @click="toggleBannerVideoSound"
    >
      <v-icon :icon="bannerVideoSoundIcon" size="22" aria-hidden="true" />
    </button>

    <img
      v-if="activeBanner.imageUrl"
      class="home-hero-banner__logo-background"
      :src="activeBanner.imageUrl"
      alt=""
      aria-hidden="true"
    >

    <div class="home-hero-banner__backdrop" aria-hidden="true" />

    <div class="home-hero-banner__panel">
      <div class="home-hero-banner__content">
        <div class="home-hero-banner__text">
          <img
            v-if="activeBanner.logoUrl"
            class="home-hero-banner__logo"
            :src="activeBanner.logoUrl"
            :alt="activeBanner.title ?? t('catalog.contentLogo')"
          >

          <p v-if="activeBanner.subtitle" class="home-hero-banner__eyebrow">
            {{ activeBanner.subtitle }}
          </p>

          <h1 v-if="activeBanner.title" class="home-hero-banner__title">
            {{ activeBanner.title }}
          </h1>

          <p v-if="activeBanner.description" class="home-hero-banner__description">
            {{ activeBanner.description }}
          </p>
        </div>

        <button
          v-if="activeBannerTarget"
          class="home-hero-banner__action"
          type="button"
          @click="handleSelectBanner"
        >
          {{ t('catalog.viewDetails') }}
        </button>
       </div>
     </div>

<div v-if="props.banners.length > 1" class="home-hero-banner__controls">
       <button
         class="home-hero-banner__arrow"
         type="button"
         :aria-label="t('catalog.previousBanner')"
         @click="showPreviousBanner"
       >
         <v-icon icon="$NavigateBefore" size="24" aria-hidden="true" />
       </button>

       <div class="home-hero-banner__dots" :aria-label="t('catalog.bannerNavigation')">
         <button
           v-for="(banner, index) in props.banners"
           :key="banner.id"
           class="home-hero-banner__dot"
           :class="{ 'home-hero-banner__dot--active': index === activeIndex }"
           type="button"
           :aria-label="t('catalog.showBanner', { index: index + 1 })"
           :aria-pressed="index === activeIndex"
           @click="showBanner(index)"
         />
       </div>

       <button
         class="home-hero-banner__arrow"
         type="button"
         :aria-label="t('catalog.nextBanner')"
         @click="showNextBanner"
       >
         <v-icon icon="$NavigateNext" size="24" aria-hidden="true" />
       </button>
     </div>
  </section>
</template>

<style scoped>
.home-hero-banner {
  --home-hero-banner-height: clamp(380px, 40vw, 520px);
  --home-hero-banner-text-outline: rgba(0, 0, 0, 0.5);
  --home-hero-banner-action-offset-x: -130px;
  position: relative;
  isolation: isolate;
  display: grid;
  grid-template-columns: 1fr;
  min-height: var(--home-hero-banner-height);
  height: var(--home-hero-banner-height);
  border: 1px solid var(--border-color-primary);
  border-radius: calc(var(--radius) + 6px);
  overflow: hidden;
  background-color: rgba(10, 14, 20, 1);
  background-image: linear-gradient(140deg, rgba(12, 17, 25, 0.98), rgba(24, 35, 47, 0.92));
  box-shadow:
    var(--shadow-heavy),
    var(--inset-light);
}

.home-hero-banner::after {
  content: '';
  position: absolute;
  inset: 0;
  z-index: 3;
  pointer-events: none;
  box-shadow: inset 0 0 20px rgba(0, 0, 0, 1);
}

:deep(.home-hero-banner__image),
:deep(.home-hero-banner__logo-background),
:deep(.home-hero-banner__backdrop) {
  position: absolute;
  inset: 0;
}

.home-hero-banner__image {
  z-index: 1;
  width: 100%;
  height: 100%;
  object-fit: contain;
  object-position: right center;
  pointer-events: none;
  user-select: none;
}

.home-hero-banner :deep(.home-hero-banner__video) {
  position: absolute;
  z-index: 2;
  top: 50%;
  right: 0;
  bottom: auto;
  left: auto;
  min-width: 0;
  min-height: 0;
  max-width: 100%;
  max-height: 100%;
  border: 0;
  background: transparent;
  overflow: hidden;
  pointer-events: none;
  opacity: 1;
  transform: translateY(-50%);
  transform-origin: right center;
  will-change: auto;
}

.home-hero-banner :deep(.home-hero-banner__video .arachnea-videojs-theme) {
  padding: 0;
  background: transparent;
}

.home-hero-banner :deep(.home-hero-banner__video .video-js),
.home-hero-banner :deep(.home-hero-banner__video .videojs-media-element),
.home-hero-banner :deep(.home-hero-banner__video .vjs-tech),
.home-hero-banner :deep(.home-hero-banner__video video) {
  background: transparent;
}

.home-hero-banner :deep(.home-hero-banner__video .vjs-poster) {
  background-color: transparent;
  background-position: right center;
  background-size: contain;
}

.home-hero-banner :deep(.home-hero-banner__video .vjs-poster img),
.home-hero-banner :deep(.home-hero-banner__video .vjs-tech),
.home-hero-banner :deep(.home-hero-banner__video video) {
  object-fit: contain;
  object-position: right center;
}

.home-hero-banner :deep(.home-hero-banner__video--frame) {
  border: 0;
  background: transparent;
}

.home-hero-banner__logo-background {
  z-index: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
  object-position: center;
  opacity: .8;
  filter: var(--backdrop-filter-strong);
  pointer-events: none;
  user-select: none;
}

.home-hero-banner__backdrop {
  z-index: 3;
  background: transparent;
}

.home-hero-banner__panel {
  position: relative;
  z-index: 4;
  display: grid;
  grid-template-rows: minmax(0, 1fr) auto;
  gap: 16px;
  min-width: 0;
  min-height: 0;
  padding: clamp(20px, 3vw, 34px);
  background: transparent;
  border-left: 0;
}

.home-hero-banner__content {
  display: flex;
  flex-direction: column;
  gap: 14px;
  max-width: 100%;
  height: 100%;
  max-height: 100%;
  min-height: 0;
  overflow: auto;
  padding-right: 6px;
}

.home-hero-banner__logo {
  max-width: min(100%, 480px);
  max-height: 130px;
  width: auto;
  height: auto;
  object-fit: contain;
  object-position: left center;
  filter: drop-shadow(0 6px 18px rgba(0, 0, 0, 0.45));
}

.home-hero-banner__eyebrow {
  color: rgba(221, 233, 245, 0.88);
  font-size: 0.88rem;
  font-weight: 700;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  -webkit-text-stroke: 0.45px var(--home-hero-banner-text-outline);
  text-shadow:
    -1px -1px 0 var(--home-hero-banner-text-outline),
    1px -1px 0 var(--home-hero-banner-text-outline),
    -1px 1px 0 var(--home-hero-banner-text-outline),
    1px 1px 0 var(--home-hero-banner-text-outline);
}

.home-hero-banner__title {
  margin: 0;
  color: var(--text-primary);
  font-size: clamp(2rem, 1.45rem + 2.2vw, 3.5rem);
  font-weight: 800;
  line-height: 1.02;
  -webkit-text-stroke: 0.7px var(--home-hero-banner-text-outline);
  text-shadow:
    -1px -1px 0 var(--home-hero-banner-text-outline),
    1px -1px 0 var(--home-hero-banner-text-outline),
    -1px 1px 0 var(--home-hero-banner-text-outline),
    1px 1px 0 var(--home-hero-banner-text-outline),
    0 2px 10px rgba(6, 10, 16, 0.55);
}

.home-hero-banner__description {
  color: var(--text-primary);
  font-size: clamp(0.98rem, 0.92rem + 0.24vw, 1.08rem);
  line-height: 1.5;
  -webkit-text-stroke: 0.4px var(--home-hero-banner-text-outline);
  text-shadow:
    -1px -1px 0 var(--home-hero-banner-text-outline),
    1px -1px 0 var(--home-hero-banner-text-outline),
    -1px 1px 0 var(--home-hero-banner-text-outline),
    1px 1px 0 var(--home-hero-banner-text-outline),
    0 2px 10px rgba(6, 10, 16, 0.55);
}

.home-hero-banner__action {
  margin-top: auto;
  align-self: start;
  width: fit-content;
  min-height: var(--control-height);
  padding: 0 20px;
  border: 0;
  border-radius: var(--radius);
  background: var(--bg-accent-blue);
  color: var(--text-primary);
  font: inherit;
  font-size: 0.98rem;
  font-weight: 700;
  letter-spacing: 0.01em;
  cursor: pointer;
  white-space: nowrap;
}

.home-hero-banner__action:hover {
  filter: brightness(1.04);
}

.home-hero-banner__action:focus-visible,
.home-hero-banner__sound-toggle:focus-visible,
.home-hero-banner__arrow:focus-visible,
.home-hero-banner__dot:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.home-hero-banner__sound-toggle {
  position: absolute;
  top: 14px;
  right: 14px;
  z-index: 11;
  display: inline-grid;
  place-items: center;
  width: 44px;
  height: 44px;
  padding: 0;
  border: 1px solid rgba(255, 255, 255, 0.22);
  border-radius: 50%;
  background: rgba(10, 14, 20, 0.50);
  color: var(--text-primary);
  cursor: pointer;
  box-shadow: var(--inset-light);
  transition:
    transform var(--duration-fast) ease,
    border-color var(--duration-fast) ease,
    background-color var(--duration-fast) ease;
}

.home-hero-banner__sound-toggle:hover {
  transform: translateY(-1px);
  border-color: var(--color-primary);
  background: rgba(10, 14, 20, 0.72);
}

.home-hero-banner__arrow {
  display: inline-grid;
  place-items: center;
  width: 44px;
  min-width: 44px;
  height: 44px;
  padding: 0;
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 50%;
  background: rgba(10, 14, 20, 0.5);
  color: var(--text-primary);
  cursor: pointer;
  transition:
    transform var(--duration-fast) ease,
    border-color var(--duration-fast) ease,
    background-color var(--duration-fast) ease;
}

.home-hero-banner__arrow:hover {
  transform: translateY(-1px);
  border-color: var(--color-primary);
}

.home-hero-banner__dots {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  flex: 0 0 auto;
  padding: 8px 18px;
  background: var(--bg-surface);
  border: 1px solid var(--border-color-primary);
  border-radius: 100px;
}

.home-hero-banner__dot {
  width: 11px;
  height: 11px;
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.28);
  cursor: pointer;
  transition:
    transform var(--duration-fast) ease,
    background-color var(--duration-fast) ease;
}

.home-hero-banner__dot--active {
  transform: scale(1.14);
  background: var(--bg-accent-blue);
}

.home-hero-banner__controls {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 20px;
  min-height: 44px;
  padding: 24px 20px 0;
  margin-top: auto;
  position: absolute;
  bottom: 14px;
  left: 0;
  right: 0;
  z-index: 10;
  pointer-events: none;
}

.home-hero-banner__arrow,
.home-hero-banner__dot {
  pointer-events: auto;
}

@media (max-width: 720px) {
  .home-hero-banner {
    --home-hero-banner-height: 520px;
    grid-template-rows: minmax(0, 1fr);
  }

  .home-hero-banner__panel {
    gap: 12px;
    padding: 14px 16px 16px;
    background: transparent;
  }

  .home-hero-banner__backdrop {
    background: transparent;
  }

  .home-hero-banner__dots {
    display: none;
  }
}
</style>
