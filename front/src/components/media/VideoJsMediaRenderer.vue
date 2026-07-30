<script setup lang="ts">
import { useAttrs, useTemplateRef, computed } from 'vue'

import {
  useVideoJsMediaRenderer,
  type VideoJsMediaRendererEmits,
  type VideoJsMediaRendererProps,
} from '@/composables/video/useVideoJsMediaRenderer'

/** Display size of a storyboard preview frame. */
const STORYBOARD_PREVIEW_SIZE = 15
/** Display width, in pixels, of a storyboard preview frame. */
const STORYBOARD_PREVIEW_WIDTH = 16 * STORYBOARD_PREVIEW_SIZE
/** Display height, in pixels, of a storyboard preview frame. */
const STORYBOARD_PREVIEW_HEIGHT = 9 * STORYBOARD_PREVIEW_SIZE

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
  /** Resolved VTT storyboard grid info (async, null until resolved). */
  vttStoryboardGrid,
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
 * source image and to size its tooltip. Scaling the tooltip here preserves the source
 * dimensions for cropping while rendering a fixed-size preview.
 *
 * The scale factors use content-only dimensions (without border) so that every source
 * cell renders at the exact preview size.  The 1 px border set by the plugin is scaled
 * along with the content, but the tiny difference is consistent across sources.
 *
 * `--sb-bg-src-w` preserves the sprite's declared column width. The height remains `auto` so
 * the browser keeps the image's intrinsic aspect ratio when a VTT declares trailing cues outside
 * the actual sprite image.
 *
 * `--storyboard-preview-text-scale` counter-scales the tooltip font-size so that time
 * text renders at the intended size regardless of the source cell dimensions.
 */
const storyboardPreviewStyle = computed(() => {
  const vttSize = vttStoryboardGrid.value
  const storyboard = props.source.storyboard
  const cellWidth = vttSize?.width ?? storyboard?.width
  const cellHeight = vttSize?.height ?? storyboard?.height

  if (!cellWidth || !cellHeight) {
    return undefined
  }

  const cols = vttSize?.columns ?? storyboard?.columns
  return {
    '--storyboard-preview-scale-x': String(STORYBOARD_PREVIEW_WIDTH / cellWidth),
    '--storyboard-preview-scale-y': String(STORYBOARD_PREVIEW_HEIGHT / cellHeight),
    '--storyboard-preview-text-scale': String(cellWidth / STORYBOARD_PREVIEW_WIDTH),
    ...(cols ? {
      '--sb-bg-src-w': String(cellWidth * cols),
    } : {}),
  }
})
</script>

<template>
  <div
    ref="hostElement"
    v-bind="mergedAttrs"
    class="videojs-media-host"
    :style="storyboardPreviewStyle"
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
      >
    </div>
  </div>
</template>

<style scoped>
.videojs-media-host {
  --videojs-control-bar-height: 55px;
  --videojs-control-gap: 0.15rem;
  --videojs-control-padding: 0.45rem;
  --videojs-icon-control-width: 2.85rem;
  --videojs-player-font-size: 1rem;
  --videojs-time-line-height: 50px;
  position: relative;
  width: 100%;
  height: auto;
  aspect-ratio: 16 / 9;
  border-radius: inherit;
  overflow: hidden;
}

.videojs-media-host:fullscreen {
  width: 100vw;
  height: 100vh;
  aspect-ratio: auto;
  border-radius: 0;
}

.videojs-media-overlay {
  position: absolute;
  inset: 0;
  z-index: 1;
  display: grid;
  place-items: center;
  padding: 24px;
  pointer-events: none;
  opacity: 0;
  transition: opacity 1s ease;
}

.videojs-media-overlay--visible {
  opacity: 1;
}

.videojs-media-overlay__logo {
  width: auto;
  height: min(66%, calc(100% - 48px));
  max-width: calc(100% - 32px);
  max-height: clamp(168px, 66%, 440px);
  object-fit: contain;
  filter:
    drop-shadow(0 10px 24px rgb(0 0 0 / 0.44))
    drop-shadow(0 4px 10px rgb(0 0 0 / 0.34));
}

.videojs-media-element,
.videojs-media-host :deep(.video-js),
.videojs-media-host :deep(.vjs-tech),
.videojs-media-host :deep(video) {
  width: 100%;
  height: 100%;
}

.videojs-media-host :deep(.video-js) {
  font-family:
    'Avenir Next',
    'Montserrat',
    'Segoe UI',
    sans-serif;
  background: transparent;
}

.videojs-media-host :deep(.arachnea-videojs-theme) {
  --player-font-size: var(--videojs-player-font-size);
  color: var(--text-primary);
  border-radius: inherit;
  padding: 0 0.15rem;
  background:
    radial-gradient(circle at top, rgb(70 163 255 / 0.14), transparent 45%),
    linear-gradient(180deg, rgb(8 12 18 / 0.12), rgb(8 12 18 / 0.4));
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-poster) {
  overflow: hidden;
  background-position: center;
  background-size: cover;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-poster img) {
  object-fit: cover;
  object-position: center;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-control-bar) {
  height: var(--videojs-control-bar-height);
  gap: var(--videojs-control-gap);
  padding-top: 5px;
  padding-right: var(--videojs-control-padding);
  padding-left: var(--videojs-control-padding);
  background: var(--bg-surface-strong);
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-progress-control) {
  position: absolute;
  top: 0;
  right: 0;
  left: 0;
  width: 100%;
  height: 18px;
  min-width: 0;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-progress-control .vjs-progress-holder) {
  position: absolute;
  top: 0;
  right: 0;
  left: 0;
  width: 100%;
  height: 0.28rem;
  margin: 0;
  background: rgb(255 255 255 / 0.2);
  transition: height 0.14s ease;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-progress-control:hover .vjs-progress-holder),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-progress-control:focus-within .vjs-progress-holder) {
  height: 0.48rem;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-progress-control .vjs-load-progress),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-progress-control .vjs-play-progress) {
  height: 100%;
}

.vjs-theme-city :deep(.vjs-play-progress) {
    background-color: var(--color-primary);
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-progress-control .vjs-play-progress::before) {
  top: 50%;
  right: -0.55rem;
  display: block;
  width: 1.1rem;
  height: 1.1rem;
  border-radius: 50%;
  background: var(--bg-accent-primary);
  content: '';
  opacity: 0;
  transform: translateY(-50%);
  transition: opacity 0.14s ease;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-progress-control:hover .vjs-play-progress::before),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-progress-control:focus-within .vjs-play-progress::before) {
  opacity: 1;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-play-control) {
  order: 1;
  flex: 0 0 auto;
  width: var(--videojs-icon-control-width);
  font-size: var(--player-font-size);
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-panel) {
  order: 3;
  display: flex !important;
  flex: 0 0 auto;
  width: auto !important;
  height: var(--videojs-time-line-height);
  margin: 0 0.9rem 0 0;
  padding-top: 0 !important;
  overflow: visible;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-control.vjs-volume-horizontal) {
  position: relative;
  z-index: 1;
  flex: 0 0 0 !important;
  height: var(--videojs-time-line-height);
  min-width: 0;
  margin: 0 !important;
  opacity: 0;
  pointer-events: none;
  overflow: hidden;
  transition:
    opacity 0.2s ease 1s,
    flex 0.2s ease 1s;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-panel:hover .vjs-volume-control.vjs-volume-horizontal) {
  flex: 0 0 5.15rem !important;
  width: 5.15rem !important;
  opacity: 1;
  pointer-events: auto;
  visibility: visible;
  transition:
    opacity 0.1s ease,
    flex 0.1s ease,
    width 0.1s ease;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-level::after) {
  position: absolute;
  top: 50%;
  right: -0.55rem;
  z-index: 1;
  width: 1.1rem;
  height: 1.1rem;
  border-radius: 50%;
  background: var(--bg-accent-primary);
  content: '';
  transform: translateY(-50%);
  opacity: 0;
  transition: opacity var(--duration-fast) ease !important;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-panel:hover .vjs-volume-level::after) {
  opacity: 1;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-mute-control) {
  position: relative;
  z-index: 2;
  display: block !important;
  flex: 0 0 2.35rem;
  height: var(--videojs-time-line-height);
  margin: 0 0 0 0;
  background: transparent;
  top: 2px;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-audio-button .vjs-icon-placeholder::before) {
  font-size: 2.1em;   /* encore plus gros */
}
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-subs-caps-button .vjs-icon-placeholder::before),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-subtitles-button .vjs-icon-placeholder::before) {
     font-size: 2.4em;   /* encore plus gros */
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-mute-control::before) {
  position: absolute;
  top: 50%;
  left: 55%;
  width: 1.7rem;
  height: 1.7rem;
  background-image: url("@/assets/icons/volume-mute.svg"); 
  content: '';
  transform: translate(-50%, -50%);
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-mute-control.vjs-vol-0::before) {
  background-image: url("@/assets/icons/volume-mute.svg");
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-mute-control.vjs-vol-1::before) {
  background-image: url("@/assets/icons/volume-low.svg");
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-mute-control.vjs-vol-2::before) {
  background-image: url("@/assets/icons/volume-medium.svg");
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-mute-control.vjs-vol-3::before) {
  background-image: url("@/assets/icons/volume-high.svg");
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-mute-control .vjs-icon-placeholder::before),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-mute-control .vjs-icon-placeholder::after),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-mute-control .vjs-svg-icon) {
  display: none !important;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-panel .vjs-volume-control.vjs-volume-horizontal) {
  position: relative;
  z-index: 1;
  flex: 1 1 auto;
  width: auto !important;
  height: var(--videojs-time-line-height);
  min-width: 0;
  margin: 0 !important;
  opacity: 1 !important;
  overflow: visible;
  visibility: visible !important;
  transition: none;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-bar.vjs-slider-horizontal) {
  position: absolute;
  top: 50%;
  right: 0;
  left: 0;
  width: auto;
  height: 0.48rem;
  margin: 0 !important;
  border-radius: 0;
  background: rgb(255 255 255 / 0.2);
  transform: translateY(-50%);
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-bar::before) {
  display: none !important;
  content: none !important;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-level) {
  height: 100%;
  border-radius: 0;
  background: var(--color-primary);
  overflow: visible;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-level::before),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-level .vjs-svg-icon) {
  display: none !important;
  content: none !important;
}



.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-tooltip),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-volume-control .vjs-mouse-display) {
  display: none !important;
}

.videojs-media-host :deep(.arachnea-videojs-theme.arachnea-live-stream .vjs-current-time) {
  cursor: default;
}

.videojs-media-host :deep(.arachnea-videojs-theme.arachnea-live-stream .vjs-time-divider),
.videojs-media-host :deep(.arachnea-videojs-theme.arachnea-live-stream .vjs-duration) {
  display: none;
}

.videojs-media-host :deep(.arachnea-videojs-theme.arachnea-live-stream .vjs-play-progress) {
  background-color: var(--bg-accent-primary);
  transition: width 0.1s ease;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-live-control),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-seek-to-live-control) {
  order: 4;
  flex: 0 0 auto;
  min-width: 0;
  padding: 10 var(--videojs-control-padding);
  line-height: var(--videojs-time-line-height);
  font-size: 1.3em;
  font-weight: 600;
}


.videojs-media-host :deep(.arachnea-videojs-theme .vjs-current-time),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-remaining-time) {
  order: 10;
  flex: 0 0 auto;
  min-width: 0;
  padding: 0 var(--videojs-control-padding);
  line-height: var(--videojs-time-line-height);
  text-align: left;
  font-size: var(--player-font-size);
  font-variant-numeric: tabular-nums;
  cursor: pointer;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-time-divider) {
  min-width: 0;
  padding: 0;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-time-divider),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-duration) {
  order: 11;
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  line-height: var(--videojs-time-line-height);
  font-size: var(--player-font-size);
  font-variant-numeric: tabular-nums;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-duration) {
  padding: 0 0.15rem;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-custom-control-spacer) {
  display: none;
}

.videojs-media-host :deep(.arachnea-videojs-theme:not(.arachnea-show-remaining-time) .vjs-current-time) {
  display: flex;
}

.videojs-media-host :deep(.arachnea-videojs-theme:not(.arachnea-show-remaining-time) .vjs-remaining-time) {
  display: none;
}

.videojs-media-host :deep(.arachnea-videojs-theme.arachnea-show-remaining-time .vjs-current-time) {
  display: none;
}

.videojs-media-host :deep(.arachnea-videojs-theme.arachnea-show-remaining-time .vjs-remaining-time) {
  display: flex;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-quality-menu-wrapper),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-quality-menu-button) {
  order: 50;
  flex: 0 0 auto;
  width: var(--videojs-icon-control-width);
  font-size: 1.08em;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-subs-caps-button) {
  order: 45;
  flex: 0 0 auto;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-audio-button) {
  order: 46;
  flex: 0 0 auto;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-prev-video-control),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-next-video-control) {
  order: 2;
  flex: 0 0 auto;
  width: var(--videojs-icon-control-width);
  font-size: var(--player-font-size);
  position: relative;
  display: flex !important;
  align-items: center;
  justify-content: center;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-prev-video-control:hover),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-next-video-control:hover) {
  filter: brightness(1.06);
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-prev-video-control--hidden),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-next-video-control--hidden) {
  display: none;
  width: 0;
  margin: 0;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-prev-video-control::before) {
  content: '';
  display: block;
  width: 24px;
  height: 24px;
  background-image: url("@/assets/icons/previous-video.svg");
  background-size: contain;
  background-repeat: no-repeat;
  background-position: center;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-next-video-control::before) {
  content: '';
  display: block;
  width: 24px;
  height: 24px;
  background-image: url("@/assets/icons/next-video.svg");
  background-size: contain;
  background-repeat: no-repeat;
  background-position: center;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-prev-video-control .vjs-icon-placeholder),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-next-video-control .vjs-icon-placeholder),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-prev-video-control .vjs-svg-icon),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-next-video-control .vjs-svg-icon) {
  display: none !important;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-episode-autoplay-toggle) {
  order: 39;
  flex: 0 0 auto;
  display: flex !important;
  align-items: center;
  justify-content: center;
  width: auto !important;
  min-width: 44px;
  margin-left: auto;
  padding: 0 10px;
  background: transparent !important;
  cursor: pointer;
}

.videojs-media-host :deep(.vjs-episode-autoplay-track) {
  position: relative;
  width: 36px;
  height: 14px;
  border-radius: 7px;
  background: rgba(255, 255, 255, 0.2);
  transition: background-color var(--duration-fast) ease;
}

.videojs-media-host :deep(.vjs-episode-autoplay-toggle--active .vjs-episode-autoplay-track) {
  background: var(--bg-accent-primary);
}

.videojs-media-host :deep(.vjs-episode-autoplay-toggle .vjs-episode-autoplay-thumb) {
  position: absolute;
  top: -2.5px;
  left: 0;
  width: 19px;
  height: 19px;
  border-radius: 999px !important;
  background: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  transition: transform var(--duration-fast) ease;
}

.videojs-media-host :deep(.vjs-episode-autoplay-toggle--active .vjs-episode-autoplay-thumb) {
  transform: translateX(17px);
}

.videojs-media-host :deep(.vjs-episode-autoplay-icon) {
  width: 14px;
  height: 14px;
  display: block;
  background-image: url("@/assets/icons/episode-autoplay-pause.svg");
  background-size: contain;
  background-repeat: no-repeat;
  background-position: center;
}

.videojs-media-host :deep(.vjs-episode-autoplay-toggle--active .vjs-episode-autoplay-icon) {
  background-image: url("@/assets/icons/episode-autoplay-play.svg");
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-fullscreen-control) {
  order: 50;
  flex: 0 0 auto;
  width: var(--videojs-icon-control-width);
  font-size: 1.7em;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-picture-in-picture-control) {
  order: 60;
  flex: 0 0 auto;
}

.videojs-media-host[data-vjs-initial-loading="true"] :deep(.vjs-big-play-button),
.videojs-media-host :deep(.video-js.vjs-waiting .vjs-big-play-button),
.videojs-media-host :deep(.video-js.vjs-seeking .vjs-big-play-button) {
  display: none !important;
}

.videojs-media-host :deep(.arachnea-videojs-div-control) {
  order: 20;
  flex: 1;
  min-width: 0;
}

.videojs-media-host :deep(.video-js.arachnea-videojs-theme .vjs-big-play-button) {
  position: absolute;
  top: 50%;
  left: 50%;
  z-index: 2 !important;
  width: 3.6rem !important;
  height: 3.6rem !important;
  margin: 0 !important;
  padding: 0 !important;
  font-size: var(--player-font-size);
  line-height: inherit !important;
  border: 1px solid rgb(255 255 255 / 0.16) !important;
  border-radius: 50% !important;
  box-shadow: 0 14px 26px rgb(0 0 0 / 0.34) !important;
  transform: translate(-50%, -50%) !important;
  background: var(--bg-accent-primary) !important;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-big-play-button .vjs-icon-placeholder) {
  display: flex;
  width: 100%;
  height: 100%;
  align-items: center;
  justify-content: center;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-big-play-button .vjs-icon-placeholder::before) {
  position: static;
  width: auto;
  height: auto;
  font-size: 2rem;
  line-height: 1;
}

.videojs-media-host :deep(.video-js.arachnea-videojs-theme:hover .vjs-big-play-button),
.videojs-media-host :deep(.video-js.arachnea-videojs-theme .vjs-big-play-button:focus-visible) {
  transform: translate(-50%, -50%) scale(1.03) !important;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-menu-button-popup .vjs-menu) {
  right: 0;
  bottom: calc(100% - 0.1rem - 30px);
  left: auto;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-menu .vjs-menu-content) {
  width: auto;
  min-width: 8em;
  padding: 0.45rem;
  border: 1px solid var(--border-color-primary);
  border-radius: 12px;
  background: var(--bg-surface-strong);
  box-shadow: var(--shadow-card);
  bottom: 25px;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-quality-menu-wrapper.vjs-menu-button-popup .vjs-menu),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-quality-menu-button.vjs-menu-button-popup .vjs-menu) {
  right: calc(var(--videojs-icon-control-width) * -1);
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-quality-menu-wrapper .vjs-menu .vjs-menu-content),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-quality-menu-button .vjs-menu .vjs-menu-content) {
  right: 0;
  left: auto;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-menu li) {
  text-transform: none;
  text-align: left;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-menu-button-popup .vjs-menu .vjs-menu-content .vjs-menu-item) {
  display: block !important;
  width: max-content !important;
  min-width: 100%;
  padding: 0.45rem 3.2em 0.45rem 0.65rem;
  color: var(--text-secondary);
  font-size: var(--player-font-size);
  text-align: left !important;
  white-space: nowrap;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-subs-caps-button.vjs-menu-button-popup .vjs-menu) {
  width: max-content;
  min-width: 10em;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-subs-caps-button.vjs-menu-button-popup .vjs-menu .vjs-menu-content) {
  width: max-content;
  min-width: 100%;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-subs-caps-button .vjs-menu-item-text) {
  white-space: nowrap;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-menu-item.vjs-selected) {
  color: var(--color-primary) !important;
  background: transparent;
  border-radius: var(--radius);
  font-weight: bolder;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-menu-item:hover),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-menu-item:focus-visible) {
  color: var(--text-primary) !important;
  background: var(--color-primary);
  border-radius: var(--radius);
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-quality-menu-item-sub-label) {
  width: 3.2em;
  color: var(--text-secondary);
  font-size: var(--player-font-size);
  letter-spacing: 0.06em;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-quality-menu-button-HD-flag::after),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-quality-menu-button-4K-flag::after) {
  border: 1px solid rgb(255 255 255 / 0.14);
    background: rgb(70 163 255 / 0.18);
    color: var(--text-primary);
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-time-tooltip) {
  border-radius: 999px;
  background: rgb(9 12 18 / 0.92);
  color: var(--text-primary);
  font-size: var(--player-font-size);
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-mouse-display .vjs-time-tooltip) {
  display: flex !important;
  align-items: flex-end !important;
  justify-content: center !important;
  padding-bottom: 4px !important;
  transform: translateY(-10px) scale(
    var(--storyboard-preview-scale-x, 1),
    var(--storyboard-preview-scale-y, 1)
  ) !important;
  font-size: calc(var(--player-font-size, 1rem) * var(--storyboard-preview-text-scale, 1)) !important;
  font-weight: 700 !important;
  border: 1px solid rgba(255, 255, 255, .9) !important;
  background-color: rgba(0,0,0, 0.40);
  background-size:
    calc(var(--sb-bg-src-w) * 1px)
    auto !important;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-play-progress .vjs-time-tooltip) {
  display: none;
}

.videojs-media-host :deep(.vjs-sprite-thumbnails .vjs-mouse-display .vjs-time-tooltip) {
  transform-origin: center bottom;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-button:focus-visible),
.videojs-media-host :deep(.arachnea-videojs-theme .vjs-menu-item:focus-visible) {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.videojs-media-host :deep(.arachnea-videojs-theme .vjs-text-track-display > div > div > div) {
  font-family: inherit;
  text-shadow: var(--text-shadow-primary);
}

.videojs-media-host :deep(.vjs-sprite-thumbnails),
.videojs-media-host :deep(.vjs-sprite-thumbnails *),
.videojs-media-host :deep(.vjs-thumbnail),
.videojs-media-host :deep(.vjs-thumbnail *) {
  border-radius: 5px !important;
}

.videojs-media-host :deep(.vjs-chapter-overlay) {
  position: absolute;
  z-index: 2;
  display: none;
  padding: 3px 8px;
  color: #fff;
  font-size: 0.8rem;
  font-weight: 600;
  line-height: 1.3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: none;
  transform: translateX(-50%);
  box-sizing: border-box;
  text-shadow: 1px 1px #000;
}

.videojs-media-host :deep(.vjs-chapter-overlay--visible) {
  display: block;
}

.videojs-media-host :deep(.vjs-chapter-segments) {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  overflow: hidden;
}

.videojs-media-host :deep(.vjs-chapter-segment-divider) {
  position: absolute;
  top: 0;
  width: 2px;
  height: 100%;
  background: rgb(255 255 255 / 0.5);
  transform: translateX(-1px);
}

.videojs-media-host :deep(.vjs-skip-intro-button),
.videojs-media-host :deep(.vjs-skip-outro-button) {
  display: none;
  position: absolute;
  bottom: 64px;
  right: 24px;
  padding: 8px 18px;
  font-size: 14px;
  font-weight: 600;
  color: #fff;
  background: var(--bg-surface);
  border: 1px solid rgb(255 255 255 / 0.3);
  border-radius: 6px;
  cursor: pointer;
  z-index: 2;
  white-space: nowrap;
  backdrop-filter: blur(4px);
  transition: opacity 0.2s;
}

.videojs-media-host :deep(.vjs-skip-intro-button--visible),
.videojs-media-host :deep(.vjs-skip-outro-button--visible) {
  display: block;
}

.videojs-media-host :deep(.vjs-skip-intro-button:active),
.videojs-media-host :deep(.vjs-skip-outro-button:active) {
  background: rgb(0 0 0 / 0.9);
}
</style>
