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

/** Service metadata utilities. */
const { getService } = useServiceMetadata()

/** Metadata for the service of the current media item. */
const serviceMetadata = computed(() => getService(activeBanner.value?.source))

/**
 * Returns the banner currently visible in the hero surface.
 */
const activeBanner = computed(() => props.banners[activeIndex.value] ?? null)

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
    directLink: null,
    webLink: null,
    name: null,
    lang: null,
    resolver: banner.player,
    storyboard: null,
    chapters: null,
    subtitles: [],
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

  const player = banner ? createBannerPlayer(banner) : null
  if (!player) {
    resolvedBannerMediaSource.value = null
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
        : resolveBackendStreamMediaSource({
            streamUrl: response.streamUrl[0] ?? null,
            manifestType: response.manifestType,
            licenseUrl: response.licenseUrl,
            licenseHeaders: response.licenseHeaders,
            storyboardVttUrl: response.storyboardVttUrl,
            chapters: response.chapters,
            subtitles: response.subtitles,
          })
  } catch (error) {
    if (requestId === bannerStreamRequestId) {
      resolvedBannerMediaSource.value = null
    }
  }
}, { immediate: true })

/**
 * Returns the resolved video source displayed behind the active banner.
 */
const activeBannerVideoSource = computed<ResolvedPlayerMediaSource | null>(() =>
  resolvedBannerMediaSource.value,
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

/** Current security mode ref used to gate decorative banner embeds. */
const bannerSecurityMode = useStorage().getParameters().securityMode

/**
 * Returns whether the active banner video may render under the security mode.
 *
 * Only embedded iframe sources are constrained: they play in `unsafe` mode or
 * when allowlisted. Native banner videos always play.
 */
const activeBannerVideoAllowed = computed(() =>
  isMediaSourceAllowedBySecurity(
    activeBannerPlayableVideoSource.value,
    bannerSecurityMode.value,
  ),
)

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

/** Requests deferred banners only after the hero placeholder has mounted. */
onMounted(() => {
  if (props.shouldLoadDeferredBanners) {
    emit('load-banners')
  }
})

/**
 * Observes the currently rendered hero surface, including the deferred loading placeholder.
 *
 * This watcher reconnects when the placeholder is replaced by the populated banner.
 */
watch(bannerElement, (element) => {
  bannerResizeObserver.value?.disconnect()
  bannerResizeObserver.value = null
  updateBannerSize()

  if (!element || typeof ResizeObserver === 'undefined') {
    return
  }

  const resizeObserver = new ResizeObserver((entries) => {
    updateBannerSize(entries[0])
  })

  resizeObserver.observe(element)
  bannerResizeObserver.value = resizeObserver
}, { flush: 'post' })

/** Cleans up rotation timer and resize observer when component unmounts. */
onBeforeUnmount(() => {
  stopAutoRotation()
  bannerResizeObserver.value?.disconnect()
  bannerResizeObserver.value = null
})
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
    ref="bannerElement"
    class="home-hero-banner"
    :aria-label="activeBanner.title ?? t('catalog.featuredContent')"
  >
      <img
      v-if="activeBanner.imageUrl && (!activeBannerVideoSource || !activeBannerVideoAllowed)"
        class="home-hero-banner__image"
        :src="activeBanner.imageUrl"
        alt=""
        aria-hidden="true"
      >

    <VideoPlayer
      v-if="activeBannerPlayableVideoSource && activeBannerVideoAllowed"
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
      v-if="activeBannerPlayableVideoSource && activeBannerVideoAllowed"
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
          <div class="home-hero-banner__service">
          <img v-if="serviceMetadata?.logo"
            class="source-logo-image"
            :src="serviceMetadata?.logo"
            :alt="serviceMetadata?.title ? `${serviceMetadata?.title} logo` : ''"
          />
            <HomePinButton v-if="props.showAddButton && activeBanner.source"
              :label="t('customHome.addBanners')" @click="emit('add-banners', activeBanner.source)" />
          </div>
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
