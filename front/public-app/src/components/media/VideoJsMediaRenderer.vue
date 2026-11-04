<script setup lang="ts">
import { useAttrs, useTemplateRef, computed } from 'vue'

import {
  useVideoJsMediaRenderer,
  type VideoJsMediaRendererEmits,
  type VideoJsMediaRendererProps,
} from '@/composables/video/useVideoJsMediaRenderer'
import { markImageUrlFailed, resolveImageUrl } from '@/composables/media/useFailedImageUrls'
import { useI18n } from '@/i18n'

import MediaCardDetailsContent from '@/components/media-card/MediaCardDetailsContent.vue'

defineOptions({
  inheritAttrs: false,
})

/** Component props with applied defaults. */
const props = withDefaults(defineProps<VideoJsMediaRendererProps & {
  /**
   * Controls visibility of the big play button overlay.
   * When undefined, defaults to the value of the `controls` prop.
   */
  showBigPlayButton?: boolean
  /**
   * Keeps the big play button hidden while the parent surface resolves a new source.
   */
  isExternalLoading?: boolean
}>(), {
  videoAriaLabel: null,
  poster: null,
  overlayLogoUrl: null,
  autoplay: false,
  muted: false,
  loop: false,
  controls: false,
  playsinline: false,
  preload: 'metadata',
  ariaHidden: false,
  tabIndex: null,
  initialPlaybackTime: null,
  preservePlaybackOnSourceSwitch: false,
  showEpisodeAutoplayToggle: false,
  isEpisodeAutoplayEnabled: false,
  initialPlayerState: null,
  isExternalLoading: false,
  previousVideoItem: null,
  nextVideoItem: null,
  serviceTitle: null,
})

const emit = defineEmits<VideoJsMediaRendererEmits>()
/** Component attributes. */
const attrs = useAttrs()
/** Template reference to the host element. */
const hostElement = useTemplateRef<HTMLDivElement>('hostElement')
/** Template reference to the video element. */
const videoElement = useTemplateRef<HTMLVideoElement>('videoElement')

/** Video.js media renderer composable results. */
const {
  /** Whether the poster overlay is currently visible. */
  isPosterOverlayVisible,
  /** Whether the poster overlay should be rendered. */
  shouldRenderPosterOverlay,
  /** Whether the video is in initial loading state. */
  isVideoInitialLoading,
  /** Fixed storyboard preview size derived from the sprite cell dimensions. */
  storyboardPreviewSize,
  /** Direction of the prev/next control currently previewed, or null when none is. */
  previewedNavigationDirection,
} = useVideoJsMediaRenderer({
  props,
  emit,
  hostElement,
  videoElement,
})

/** Whether the big play button should be suppressed. */
const isBigPlayButtonSuppressed = computed(() =>
  isVideoInitialLoading.value || props.isExternalLoading,
)

/**
 * Adds a data attribute while loading states should suppress the big play button.
 */
const hostDataAttr = computed(() => {
  if (!isBigPlayButtonSuppressed.value) {
    return { 'data-vjs-initial-loading': undefined }
  }
  return { 'data-vjs-initial-loading': 'true' }
})

/**
 * Apply the data attribute to the root element by merging with attrs.
 */
const mergedAttrs = computed(() => ({
  ...attrs,
  ...hostDataAttr.value,
}))

/**
 * Keeps the storyboard preview size independent from the source sprite cell dimensions.
 *
 * The sprite thumbnail plugin uses `width` and `height` both to crop a cell from the
 * source image and to size its tooltip. The tooltip layout box is resized in the
 * renderer composable instead of CSS-scaled so Video.js measures the displayed size
 * when clamping the tooltip inside the player near the seek bar edges.
 *
 * `storyboardTooltipStyle` exposes the fixed preview size as CSS variables so the
 * layout box is correct synchronously, and counter-scales the time text so it keeps
 * its intended size whatever the source cell dimensions.
 */
const storyboardTooltipStyle = computed(() => {
  const previewSize = storyboardPreviewSize.value

  if (!previewSize) {
    return undefined
  }

  return {
    '--storyboard-preview-width': `${previewSize.displayWidth}px`,
    '--storyboard-preview-height': `${previewSize.displayHeight}px`,
    '--storyboard-preview-background-width': previewSize.columns > 0
      ? `${previewSize.displayWidth * previewSize.columns}px`
      : undefined,
    '--storyboard-preview-display-to-source-scale': String(previewSize.displayToSourceScale),
  }
})

/** Internationalization utilities. */
const { t } = useI18n()

/**
 * Media item previewed by the adjacent-navigation control currently hovered or focused.
 */
const previewedNavigationItem = computed(() => {
  if (previewedNavigationDirection.value === -1) {
    return props.previousVideoItem ?? null
  }

  if (previewedNavigationDirection.value === 1) {
    return props.nextVideoItem ?? null
  }

  return null
})

/** Label describing the navigation direction of the previewed adjacent item. */
const previewedNavigationLabel = computed(() => {
  if (!previewedNavigationItem.value) {
    return null
  }

  return t(
    previewedNavigationDirection.value === -1 ? 'entry.previousVideo' : 'entry.nextVideo',
  )
})

/** Title of the previewed adjacent item, or null when the backend does not provide one. */
const previewedNavigationTitle = computed(() =>
  previewedNavigationItem.value?.title?.trim() || null,
)

/** Poster of the previewed adjacent item, or null when no image can be resolved. */
const previewedNavigationImageUrl = computed(() => {
  const item = previewedNavigationItem.value

  if (!item) {
    return null
  }

  return resolveImageUrl([
    item.imageLandscapeUrl,
    item.imagePosterUrl,
    item.imagePortraitUrl,
    item.imageUrl,
  ])
})
</script>

<template>
  <div
    ref="hostElement"
    v-bind="mergedAttrs"
    class="videojs-media-host"
    :style="storyboardTooltipStyle"
  >
    <video
      ref="videoElement"
      class="video-js videojs-media-element vjs-big-play-centered vjs-theme-city arachnea-videojs-theme"
    />

    <div
      v-if="shouldRenderPosterOverlay && props.overlayLogoUrl"
      class="videojs-media-overlay"
      :class="{ 'videojs-media-overlay--visible': isPosterOverlayVisible }"
      aria-hidden="true"
    >
      <img
        class="videojs-media-overlay__logo"
        :src="props.overlayLogoUrl"
        alt=""
        @error="markImageUrlFailed(props.overlayLogoUrl)"
      >
    </div>

    <div
      v-if="previewedNavigationItem && previewedNavigationLabel"
      class="videojs-media-navigation-preview"
      role="tooltip"
    >
      <img
        v-if="previewedNavigationImageUrl"
        class="videojs-media-navigation-preview__image"
        :src="previewedNavigationImageUrl"
        alt=""
        @error="markImageUrlFailed(previewedNavigationImageUrl)"
      >

      <div class="videojs-media-navigation-preview__copy">
        <p class="videojs-media-navigation-preview__label">
          {{ previewedNavigationLabel }}
        </p>

        <h3
          v-if="previewedNavigationTitle"
          class="videojs-media-navigation-preview__title"
        >
          {{ previewedNavigationTitle }}
        </h3>

        <MediaCardDetailsContent
          :item="previewedNavigationItem"
          :service-title="props.serviceTitle"
          :hide-source="true"
        />
      </div>
    </div>
  </div>
</template>

<style scoped src="./video-js/host.css"></style>
<style scoped src="./video-js/control-bar.css"></style>
<style scoped src="./video-js/menus.css"></style>
<style scoped src="./video-js/chapters.css"></style>
<style scoped src="./video-js/navigation-preview.css"></style>
