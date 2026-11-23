<script setup lang="ts">
import HomePinButton from '@/components/home/HomePinButton.vue'
import { computed, onBeforeUnmount, onMounted, shallowRef, useTemplateRef, watch } from 'vue'
import { useRouter } from 'vue-router'

import {
  resolveBackendStreamMediaSource,
  resolveBackgroundMediaSource,
  resolveIframeMediaSource,
  type ResolvedPlayerMediaSource,
} from '@/services/players'
import { getStream } from '@/services/rustify'
import { useStorage } from '@/services/storage'
import { STORE_PREFIX } from '@/services/storage/keys'
import { isMediaSourceAllowedBySecurity } from '@/services/videoSourceAllowlist'
import { mediaPresentation, mediaRoute } from '@/services/mediaRouting'
import VideoPlayer from '@/components/media/VideoPlayer.vue'
import type { VideoJsMediaDimensions } from '@/composables/video/useVideoJsMediaRenderer'
import { useI18n } from '@/i18n'
import type { HomeBanner } from '@/types/home'
import type { EntryPlayer } from '@/types/entry'
import type { MediaSelectionTarget } from '@/types/media'
import { useServiceMetadata } from '@/composables/useServiceMetadata'

/**
 * Props accepted by the featured banner carousel.
 */
interface Props {
  /**
   * Banners exposed by the backend catalog payload.
   */
  banners: HomeBanner[]
  /**
   * Indicates that deferred banners should be requested after this component mounts.
   * @default false
   */
  shouldLoadDeferredBanners?: boolean
  /** Whether source-collection additions are available in edition mode. @default false */
  showAddButton?: boolean
}

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
  shouldLoadDeferredBanners: false,
  showAddButton: false,
})

const emit = defineEmits<{
  /** Emitted when a media item is selected from a banner. */
  'select-item': [target: MediaSelectionTarget]
  /** Emitted after mount to request deferred banner collections. */
  'load-banners': []
  'add-banners': [sourceId: string]
}>()

/** Delay in milliseconds between automatic banner rotations. */
const AUTO_ROTATION_DELAY_MS = 20000
/** Maximum number of banner indicators visible at once. */
const MAX_VISIBLE_INDICATORS = 10
/** Default aspect ratio for banner videos. */
const DEFAULT_BANNER_VIDEO_ASPECT_RATIO = 16 / 9
const BANNER_VIDEO_PAUSED_KEY = `${STORE_PREFIX}home.bannerVideo.paused`

/** Identifier used to ignore stale banner stream resolutions. */
let bannerStreamRequestId = 0

/** Index of the currently active banner. */
const activeIndex = shallowRef(0)
/** Number of indicators in the scrolling navigation window. */
const visibleIndicatorCount = computed(() => Math.min(props.banners.length, MAX_VISIBLE_INDICATORS))
/** First visible indicator, keeping the active banner near the center when possible. */
const indicatorWindowStart = computed(() => Math.max(0, Math.min(
  activeIndex.value - Math.floor((MAX_VISIBLE_INDICATORS - 1) / 2),
  props.banners.length - MAX_VISIBLE_INDICATORS,
)))
/** Visible indicators retain their absolute indices when asynchronous sources extend the list. */
const visibleIndicators = computed(() => props.banners
  .slice(indicatorWindowStart.value, indicatorWindowStart.value + visibleIndicatorCount.value)
  .map((banner, offset) => ({ banner, index: indicatorWindowStart.value + offset })))
/** Timer ID for the automatic rotation. */
const autoRotationTimer = shallowRef<ReturnType<typeof window.setTimeout> | null>(null)
const bannerVideoPlayer = useTemplateRef<InstanceType<typeof VideoPlayer>>('bannerVideoPlayer')
/** Current video aspect ratio for the active banner. */
const bannerVideoAspectRatio = shallowRef(DEFAULT_BANNER_VIDEO_ASPECT_RATIO)
/** Whether the banner video is muted. */
const isBannerVideoMuted = shallowRef(true)
const isBannerVideoPaused = shallowRef(readBannerVideoPaused())
/** Internationalization utilities. */
const { t } = useI18n()

/** Service metadata utilities. */
const { getService } = useServiceMetadata()

/** Metadata for the service of the current media item. */
const serviceMetadata = computed(() => getService(activeBanner.value?.source))

/**
 * Returns the banner currently visible in the hero surface.
 */
const activeBanner = computed(() => props.banners[activeIndex.value] ?? null)

/** Channel branding takes the provider logo's slot, with a fallback for broken images. */
const failedChannelLogoUrl = shallowRef<string | null>(null)
const channelLogoUrl = computed(() => {
  const url = activeBanner.value?.sourceLogoUrl
  return url && url !== failedChannelLogoUrl.value ? url : null
})
const sourceLogoUrl = computed(() => channelLogoUrl.value ?? serviceMetadata.value?.logo)

/** @param event Image error from the logo in the provider slot. */
function handleSourceLogoError(event: Event) {
  const image = event.currentTarget
  if (image instanceof HTMLImageElement && image.getAttribute('src') === channelLogoUrl.value) {
    failedChannelLogoUrl.value = channelLogoUrl.value
  }
}

/**
 * Resolved media source for the active banner.
 *
 * When a banner provides a `player` resolver instead of a direct `videoUrl`,
 * this ref is asynchronously populated by calling `get_stream` on the backend.
 */
const resolvedBannerMediaSource = shallowRef<ResolvedPlayerMediaSource | null>(null)

/**
 * Builds the minimal player descriptor required to resolve one banner stream.
 *
 * @param banner Banner containing an optional backend player resolver.
 * @returns Player descriptor, or null when the banner has no resolver.
 */
function createBannerPlayer(banner: HomeBanner): EntryPlayer | null {
  if (!banner.player) {
    return null
  }

  return {
    id: `${banner.id}-banner`,
    label: banner.title ?? banner.id,
    streams: [],
    imageTitleLink: null,
    storyboardVttUrl: null,
    storyboard: null,
    chapters: null,
    subtitles: [],
    webLink: null,
    name: null,
    lang: null,
    resolver: banner.player,
  }
}

/**
 * Asynchronously resolves the active banner video stream when player-based resolution is needed.
 */
watch(activeBanner, async (banner) => {
  const requestId = ++bannerStreamRequestId
  const directMediaSource = resolveBackgroundMediaSource(banner?.videoUrl ?? null)

  if (directMediaSource) {
    resolvedBannerMediaSource.value = directMediaSource
    return
  }

  resolvedBannerMediaSource.value = null
  const player = banner ? createBannerPlayer(banner) : null
  if (!player) {
    return
  }

  try {
    const response = await getStream(player)
    if (requestId !== bannerStreamRequestId) {
      return
    }

    resolvedBannerMediaSource.value = !response
      ? null
      : 'embedLink' in response
        ? resolveIframeMediaSource(response.embedLink)
        : resolveBackendStreamMediaSource(response)
  } catch (error) {
    if (requestId === bannerStreamRequestId) {
      resolvedBannerMediaSource.value = null
    }
  }
}, { immediate: true })

/**
 * Returns the banner video source configured with the current banner-only sound preference.
 */
const activeBannerPlayableVideoSource = computed<ResolvedPlayerMediaSource | null>(() => {
  const source = resolvedBannerMediaSource.value

  if (!source || source.renderer !== 'iframe') {
    return source
  }

  try {
    const url = new URL(source.src)
    url.searchParams.set('mute', isBannerVideoMuted.value ? '1' : '0')
    url.searchParams.set('autoplay', '1')

    return {
      ...source,
      src: url.toString(),
    }
  } catch {
    return source
  }
})

/** Current security mode ref used to gate decorative banner embeds. */
const bannerSecurityMode = useStorage().getParameters().securityMode

/**
 * Returns the active video if the current security mode permits its source.
 *
 * Only embedded iframe sources are constrained: they are allowed in `unsafe`
 * mode or when allowlisted.
 */
const activeBannerVideo = computed(() => {
  const source = activeBannerPlayableVideoSource.value
  return source && isMediaSourceAllowedBySecurity(
    source,
    bannerSecurityMode.value,
  ) ? source : null
})

const shouldRenderBannerVideo = computed(() =>
  activeBannerVideo.value?.renderer === 'video' || !isBannerVideoPaused.value,
)

/**
 * Returns a stable key that forces the active banner media element to remount when it changes.
 */
const activeBannerMediaKey = computed(() => {
  const banner = activeBanner.value
  const source = activeBannerVideo.value

  if (!banner || !source) {
    return null
  }

  return `${banner.id}:${source.renderer}:${source.src}`
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
  activeBannerVideo.value?.renderer === 'video',
)

/**
 * Indicates whether the active banner should rotate on a timer.
 */
const shouldUseTimedAutoRotation = computed(() =>
  props.banners.length > 1 &&
  !shouldRotateActiveBannerOnVideoEnd.value &&
  !(activeBannerVideo.value && isBannerVideoPaused.value),
)

/**
 * Indicates whether the active banner video should loop instead of rotating to another banner.
 */
const shouldLoopActiveBannerVideo = computed(() =>
  !shouldRotateActiveBannerOnVideoEnd.value,
)

/**
 * Returns the type-driven target for the active banner, including legacy entry targets.
 */
const activeBannerTarget = computed<MediaSelectionTarget | null>(() => {
  const banner = activeBanner.value
  if (banner?.target) return banner.target
  const entryUrl = banner?.entryUrl ?? banner?.webUrl ?? null

  if (!banner?.source || !entryUrl) {
    return null
  }

  return {
    source: banner.source,
    entryUrl,
    webUrl: banner.webUrl ?? entryUrl,
    title: banner.title,
  }
})

/** Vue Router instance used to resolve the banner action link. */
const router = useRouter()

/**
 * Resolves the registered route targeted by the banner action.
 */
const bannerActionHref = computed(() => {
  const target = activeBannerTarget.value
  const destination = target ? mediaRoute(target) : null
  return destination ? router.resolve(destination).href : undefined
})

/** Action label selected by the same registry as the banner route. */
const bannerActionLabel = computed(() => activeBannerTarget.value
  ? mediaPresentation(activeBannerTarget.value).bannerActionLabel : 'catalog.viewDetails')

watch(
  () => props.banners,
  (banners, previous) => {
    const length = banners.length
    if (length === 0) {
      activeIndex.value = 0
      return
    }

    const previousBanner = previous?.[activeIndex.value]
    const retainedIndex = previousBanner ? banners.findIndex((banner) =>
      banner.id === previousBanner.id && banner.source === previousBanner.source) : -1
    activeIndex.value = retainedIndex >= 0 ? retainedIndex : Math.min(activeIndex.value, length - 1)
  },
  { immediate: true },
)

watch(
  [() => props.banners, shouldUseTimedAutoRotation],
  () => {
    restartAutoRotation()
  },
  { immediate: true },
)

watch(activeBannerMediaKey, () => {
  bannerVideoAspectRatio.value = DEFAULT_BANNER_VIDEO_ASPECT_RATIO
})

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

/** Reads the banner playback preference from this browser. */
function readBannerVideoPaused() {
  try {
    return localStorage.getItem(BANNER_VIDEO_PAUSED_KEY) === 'true'
  } catch {
    return false
  }
}

/** Pauses or resumes the banner and remembers the choice in this browser. */
function toggleBannerVideoPlayback() {
  const paused = !isBannerVideoPaused.value
  isBannerVideoPaused.value = paused
  bannerVideoPlayer.value?.setPlaybackPaused(paused)

  try {
    localStorage.setItem(BANNER_VIDEO_PAUSED_KEY, String(paused))
  } catch {
    // Playback remains controllable when storage is unavailable.
  }
}

/**
 * Moves to the next banner once the active native banner video reaches its end.
 */
function handleBannerVideoPlaybackEnded() {
  if (isBannerVideoPaused.value || !shouldRotateActiveBannerOnVideoEnd.value) {
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
 * Emits the active banner target through the common media navigation flow.
 * @param event Mouse event, retaining native modified link navigation.
 */
function handleSelectBanner(event: MouseEvent) {
  if (event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return
  event.preventDefault()
  if (!activeBannerTarget.value) {
    return
  }

  if (mediaRoute(activeBannerTarget.value)) emit('select-item', activeBannerTarget.value)
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
 * Restarts the timer when the active banner uses timed rotation.
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

/** Requests deferred banners only after the hero placeholder has mounted. */
onMounted(() => {
  if (props.shouldLoadDeferredBanners) {
    emit('load-banners')
  }
})

onBeforeUnmount(stopAutoRotation)
</script>

<template>
  <section
    v-if="!activeBanner && props.shouldLoadDeferredBanners"
    class="home-hero-banner home-hero-banner--loading"
    :aria-label="t('catalog.featuredContent')"
    aria-busy="true"
  />

  <section
    v-else-if="activeBanner"
    class="home-hero-banner"
    :class="{ 'home-hero-banner--with-video': activeBannerVideo }"
    :style="{ '--banner-video-ratio': bannerVideoAspectRatio }"
    :aria-label="activeBanner.title ?? t('catalog.featuredContent')"
  >
    <img
      v-if="!activeBannerVideo && activeBanner.imageUrl"
      class="home-hero-banner__image"
      :src="activeBanner.imageUrl"
      alt=""
      aria-hidden="true"
    >

    <img
      v-if="activeBanner.imageUrl"
      class="home-hero-banner__logo-background"
      :src="activeBanner.imageUrl"
      alt=""
      aria-hidden="true"
    >

    <div class="home-hero-banner__backdrop" aria-hidden="true" />

    <div
      class="home-hero-banner__panel"
      :class="{ 'home-hero-banner__panel--with-navigation': activeBannerVideo && props.banners.length > 1 }"
    >
      <div class="home-hero-banner__content">
        <div class="home-hero-banner__text">
          <div class="home-hero-banner__service">
            <img
              v-if="sourceLogoUrl"
              :key="sourceLogoUrl"
              class="source-logo-image"
              :src="sourceLogoUrl"
              :alt="!channelLogoUrl && serviceMetadata?.title ? `${serviceMetadata.title} logo` : ''"
              @error="handleSourceLogoError"
            />
            <HomePinButton v-if="props.showAddButton && activeBanner.source"
              :label="t('customHome.addBanners')" @click="emit('add-banners', activeBanner.source)" />
          </div>
          <img
            v-if="activeBanner.logo"
            class="home-hero-banner__logo"
            :src="activeBanner.logo"
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

        <a
          v-if="bannerActionHref"
          class="home-hero-banner__action"
          :href="bannerActionHref"
          @click="handleSelectBanner"
        >
          {{ t(bannerActionLabel) }}
        </a>
      </div>
    </div>

    <div
      v-if="activeBannerVideo"
      class="home-hero-banner__video-area"
    >
      <VideoPlayer
        v-if="shouldRenderBannerVideo"
        ref="bannerVideoPlayer"
        :key="activeBannerMediaKey ?? undefined"
        :source="activeBannerVideo"
        class="home-hero-banner__video"
        :iframe-title="t('media.decorativeBannerVideo')"
        :poster="activeBanner.imageUrl ?? undefined"
        :autoplay="!isBannerVideoPaused"
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
      <img
        v-else-if="activeBanner.imageUrl"
        class="home-hero-banner__video-poster"
        :src="activeBanner.imageUrl"
        alt=""
      >

      <div class="home-hero-banner__video-controls">
        <button
          class="home-hero-banner__video-toggle"
          type="button"
          :aria-label="t(isBannerVideoPaused ? 'player.controls.play' : 'player.controls.pause')"
          @click="toggleBannerVideoPlayback"
        >
          <v-icon :icon="isBannerVideoPaused ? 'mdi-play' : 'mdi-pause'" size="22" aria-hidden="true" />
        </button>
        <button
          class="home-hero-banner__video-toggle"
          type="button"
          :aria-label="bannerVideoSoundLabel"
          :aria-pressed="!isBannerVideoMuted"
          @click="toggleBannerVideoSound"
        >
          <v-icon :icon="bannerVideoSoundIcon" size="22" aria-hidden="true" />
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

      <div
        class="home-hero-banner__dots"
        :aria-label="t('catalog.bannerNavigation')"
        :style="{ '--indicator-count': visibleIndicatorCount }"
      >
        <div class="home-hero-banner__dots-viewport">
          <div class="home-hero-banner__dots-track">
            <button
              v-for="{ banner, index } in visibleIndicators"
              :key="JSON.stringify([banner.source, banner.id, index])"
              class="home-hero-banner__dot"
              :class="{
                'home-hero-banner__dot--active': index === activeIndex,
              }"
              type="button"
              :aria-label="t('catalog.showBanner', { index: index + 1 })"
              :aria-pressed="index === activeIndex"
              @click="showBanner(index)"
            />
          </div>
        </div>
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

<style scoped src="./hero-banner/layout.css"></style>
<style scoped src="./hero-banner/media.css"></style>
<style scoped src="./hero-banner/panel.css"></style>
<style scoped src="./hero-banner/typography.css"></style>
<style scoped src="./hero-banner/controls.css"></style>
