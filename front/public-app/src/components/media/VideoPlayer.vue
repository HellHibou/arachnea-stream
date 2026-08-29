<script setup lang="ts">
import { useVideoPlayer } from '@/composables/video/useVideoPlayer'
import VideoJsMediaRenderer from '@/components/media/VideoJsMediaRenderer.vue'
import LoadingSpinner from '@/components/icons/LoadingSpinner.vue'
import { useI18n } from '@/i18n'
import type { ResolvedVideoMediaSource } from '@/services/players'
import type { VideoJsMediaDimensions } from '@/composables/video/useVideoJsMediaRenderer'
import { markImageUrlFailed } from '@/composables/media/useFailedImageUrls'

defineOptions({
  inheritAttrs: false,
})

/** Component props with applied defaults. */
const props = withDefaults(defineProps<{
  /**
   * Resolved media source describing how one standalone media surface should be rendered.
   * @default null
   */
  source?: import('@/services/players').ResolvedPlayerMediaSource | null
  /**
   * Accessible title applied to iframe renderers outside the entry details player mode.
   * @default null
   */
  iframeTitle?: string | null
  /**
   * Accessible label applied to interactive video renderers outside the entry details player mode.
   * @default null
   */
  videoAriaLabel?: string | null
  /**
   * Optional poster image used by standalone video renderers.
   * @default null
   */
  poster?: string | null
  /**
   * Enables autoplay on standalone video renderers when supported by the browser.
   * @default false
   */
  autoplay?: boolean
  /**
   * Mutes the standalone video surface when required by the caller.
   * @default false
   */
  muted?: boolean
  /**
   * Loops the standalone video surface when required by the caller.
   * @default false
   */
  loop?: boolean
  /**
   * Displays native controls on standalone video renderers.
   * @default false
   */
  controls?: boolean
  /**
   * Enables inline playback on mobile browsers.
   * @default false
   */
  playsinline?: boolean
  /**
   * Preload strategy applied to standalone video renderers.
   * @default 'metadata'
   */
  preload?: 'none' | 'metadata' | 'auto'
  /**
   * Loading strategy applied to standalone iframe renderers.
   * @default 'eager'
   */
  loading?: 'eager' | 'lazy'
  /**
   * Optional permissions string applied to standalone iframe renderers.
   * @default null
   */
  allow?: string | null
  /**
   * Enables fullscreen for standalone iframe renderers.
   * @default false
   */
  allowFullscreen?: boolean
  /**
   * Referrer policy applied to standalone iframe renderers.
   * @default 'strict-origin-when-cross-origin'
   */
  referrerPolicy?: 'no-referrer' | 'no-referrer-when-downgrade' | 'origin' | 'origin-when-cross-origin' | 'same-origin' | 'strict-origin' | 'strict-origin-when-cross-origin' | 'unsafe-url' | null
  /**
   * Hides standalone decorative media from accessibility APIs.
   * @default false
   */
  ariaHidden?: boolean
  /**
   * Optional tabindex forwarded to the standalone rendered element.
   * @default null
   */
  tabIndex?: number | null
  /**
   * Display title used by the entry details player mode.
   * @default null
   */
  displayTitle?: string | null
  /**
   * Indicates whether the trailer surface should be rendered by the entry details player mode.
   * @default false
   */
  showTrailerPlayer?: boolean
  /**
   * Indicates whether the selected media surface should be rendered by the entry details player mode.
   * @default false
   */
  showMediaPlayer?: boolean

  /**
   * Resolved selected media source rendered by the entry details player mode.
   * @default null
   */
  mediaSource?: import('@/services/players').ResolvedPlayerMediaSource | null
  /**
   * Poster image used by the integrated Video.js media player before playback starts.
   * @default null
   */
  mediaPosterUrl?: string | null
  /**
   * Logo image overlaid on top of the integrated Video.js poster before playback starts.
   * @default null
   */
  mediaOverlayLogoUrl?: string | null
  /**
   * Public media URL exposed by the selected entry details player.
   * @default null
   */
  mediaOpenUrl?: string | null
  /**
   * Indicates whether the entry details media source is currently being resolved.
   * @default false
   */
  isMediaPlayerLoading?: boolean
  /**
   * Error message rendered when the entry details media source resolution fails.
   * @default null
   */
  mediaPlayerErrorMessage?: string | null
  /**
   * Indicates whether selectors should be shown below the entry details media surface.
   * @default false
   */
  showPlayerControls?: boolean
  /**
   * Indicates whether the language selector should be rendered.
   * @default false
   */
  showLanguageSelector?: boolean
  /**
   * Indicates whether the player selector should be rendered.
   * @default false
   */
  showPlayerSelector?: boolean
  /**
   * Language options rendered by the entry details player selector.
   * @default []
   */
  availableLanguages?: import('@/composables/entry-details/entryVideoPlayer').EntryPlayerLanguageOption[]
  /**
   * Active language key selected in the entry details player.
   * @default null
   */
  activeLanguageKey?: string | null
  /**
   * Player options rendered by the entry details player selector.
   * @default []
   */
  filteredPlayers?: import('@/types/entry').EntryPlayer[]
  /**
   * Active player identifier selected in the entry details player.
   * @default null
   */
  activePlayerId?: string | null
  /**
   * Playback position restored for the current Video.js media surface.
   * @default null
   */
  initialPlaybackTime?: number | null
  /**
   * Enables autoplay when the integrated player switches to the next episode programmatically.
   * @default false
   */
  mediaAutoplay?: boolean
  /**
   * Keeps the last mounted media surface alive while one episode transition updates the page state.
   * @default false
   */
  preferPersistedMediaSurface?: boolean
  /**
   * Indicates whether the episode autoplay preference toggle should be rendered.
   * @default false
   */
  showEpisodeAutoplayToggle?: boolean
  /**
   * Indicates whether the episode autoplay preference is currently enabled.
   * @default false
   */
  isEpisodeAutoplayEnabled?: boolean
  /**
   * Indicates whether video navigation controls should be shown in the player control bar.
   * @default false
   */
  showVideoNavigationControls?: boolean
  /**
   * Indicates whether there is a previous video available to navigate to.
   * @default false
   */
  hasPreviousVideo?: boolean
  /**
   * Indicates whether there is a next video available to navigate to.
   * @default false
   */
  hasNextVideo?: boolean
  /** Title displayed when hovering the previous-video control. */
  previousVideoTitle?: string | null
  /** Title displayed when hovering the next-video control. */
  nextVideoTitle?: string | null
  /**
   * Mode used to render embedded iframe players.
    * @default 'safe'
   */
  securityMode?: 'unsafe' | 'confirmation' | 'safe'
}>(), {
  source: null,
  iframeTitle: null,
  videoAriaLabel: null,
  poster: null,
  autoplay: false,
  muted: false,
  loop: false,
  controls: false,
  playsinline: false,
  preload: 'metadata',
  loading: 'eager',
  allow: null,
  allowFullscreen: false,
  referrerPolicy: 'strict-origin-when-cross-origin',
  ariaHidden: false,
  tabIndex: null,
  displayTitle: null,
  showTrailerPlayer: false,
  showMediaPlayer: false,
  mediaSource: null,
  mediaPosterUrl: null,
  mediaOverlayLogoUrl: null,
  mediaOpenUrl: null,
  isMediaPlayerLoading: false,
  mediaPlayerErrorMessage: null,
  showPlayerControls: false,
  showLanguageSelector: false,
  showPlayerSelector: false,
  availableLanguages: () => [],
  activeLanguageKey: null,
  filteredPlayers: () => [],
  activePlayerId: null,
  initialPlaybackTime: null,
  mediaAutoplay: false,
  preferPersistedMediaSurface: false,
  showEpisodeAutoplayToggle: false,
  isEpisodeAutoplayEnabled: false,
  previousVideoTitle: null,
  nextVideoTitle: null,
  securityMode: 'safe',
})

const emit = defineEmits<{
  /** Emitted when the active language key changes. */
  'update:active-language-key': [value: string | null]
  /** Emitted to remember the current language selection. */
  'remember-current-language': []
  /** Emitted when the active player ID changes. */
  'update:active-player-id': [value: string | null]
  /** Emitted to remember the current player selection. */
  'remember-current-player': []
  /** Emitted when playback progress updates. */
  'update:playback-progress': [value: number | null, duration: number | null]
  /** Emitted when episode autoplay enabled state changes. */
  'update:is-episode-autoplay-enabled': [value: boolean]
  /** Emitted when media playback starts. */
  'playback-started': [sourceUrl: string | null]
  /** Emitted when media playback ends. */
  'playback-ended': []
  /** Emitted when video metadata is loaded. */
  'video-metadata-loaded': [value: VideoJsMediaDimensions]
  /** Emitted when video navigation is requested via control bar buttons. */
  'navigate-video': [direction: -1 | 1]
  /** Emitted when Video.js cannot play the active media source. */
  'source-error': []
}>()
/** Internationalization utilities. */
const { t } = useI18n()

/** Video player composable results. */
const {
  /** Persisted video player state. */
  persistedVideoPlayerState,
  /** Whether the media surface should be rendered. */
  shouldRenderSurface,
  /** Whether the loading state should be rendered. */
  shouldRenderSurfaceLoadingState,
  /** Whether the error state should be rendered. */
  shouldRenderSurfaceErrorState,
  /** CSS class for the surface container. */
  surfaceContainerClass,
  /** CSS classes for the surface state. */
  surfaceStateClasses,
  /** Key for the active surface renderer. */
  activeSurfaceRendererKey,
  /** Active iframe source for standalone rendering. */
  activeIframeSource,
  /** Active video source for Video.js rendering. */
  activeVideoSource,
  /** Attributes for standalone renderer. */
  standaloneRendererAttrs,
  /** Title for the active iframe. */
  activeIframeTitle,
  /** Allow attribute for the active iframe. */
  activeIframeAllow,
  /** Allow fullscreen for the active iframe. */
  activeIframeAllowFullscreen,
  /** Loading strategy for the active iframe. */
  activeIframeLoading,
  /** Referrer policy for the active iframe. */
  activeIframeReferrerPolicy,
  /** Aria hidden for the active iframe. */
  activeIframeAriaHidden,
  /** Tab index for the active iframe. */
  activeIframeTabIndex,
  /** CSS class for the active iframe. */
  activeIframeClass,
  /** Aria label for the active video. */
  activeVideoAriaLabel,
  /** Poster for the active video. */
  activeVideoPoster,
  /** Overlay logo URL for the active video. */
  activeVideoOverlayLogoUrl,
  /** Autoplay setting for the active video. */
  activeVideoAutoplay,
  /** Muted setting for the active video. */
  activeVideoMuted,
  /** Loop setting for the active video. */
  activeVideoLoop,
  /** Controls setting for the active video. */
  activeVideoControls,
  /** Playsinline setting for the active video. */
  activeVideoPlaysinline,
  /** Preload setting for the active video. */
  activeVideoPreload,
  /** Aria hidden setting for the active video. */
  activeVideoAriaHidden,
  /** Tab index for the active video. */
  activeVideoTabIndex,
   /** Initial playback time for the active video. */
   activeVideoInitialPlaybackTime,
   /** Whether the active video should preserve playback during its source switch. */
   activeVideoPreservePlaybackOnSourceSwitch,
  /** Whether to show episode autoplay toggle for the active video. */
  activeVideoShowEpisodeAutoplayToggle,
  /** Whether episode autoplay is enabled for the active video. */
  activeVideoIsEpisodeAutoplayEnabled,
  /** Whether to show big play button for the active video. */
  activeVideoShowBigPlayButton,
  /** CSS class for the active video. */
  activeVideoClass,
  /** Whether to show the details player picker. */
  shouldShowDetailsPlayerPicker,
  /** Language model for the player picker. */
  languageModel,
  /** Player model for the player picker. */
  playerModel,
  /** Function to handle player state updates. */
  handlePlayerStateUpdate,
  /** Function to handle language changes. */
  handleLanguageChange,
  /** Function to handle player changes. */
  handlePlayerChange,
  /** Function to handle active video playback progress updates. */
  handleActiveVideoPlaybackProgressUpdate,
  /** Function to handle active video playback start. */
  handleActiveVideoPlaybackStarted,
  /** Function to handle active video playback end. */
  handleActiveVideoPlaybackEnded,
  /** Function to handle active video episode autoplay enabled updates. */
  handleActiveVideoEpisodeAutoplayEnabledUpdate,
  /** Whether to show video navigation controls for the active video. */
  activeVideoShowVideoNavigationControls,
  /** Whether there is a previous video for the active video. */
  activeVideoHasPreviousVideo,
  /** Whether there is a next video for the active video. */
  activeVideoHasNextVideo,
  /** Function to handle video navigation. */
  handleActiveVideoVideoNavigation,
  /** Whether the iframe confirmation should be shown. */
  shouldShowIframeConfirmation,
  /** Whether the embedded iframe should be blocked. */
  shouldBlockIframe,
  /** Whether the source video action should be hidden. */
  shouldHideSourceVideoAction,
  /** Whether the iframe should be rendered. */
  shouldRenderIframe,
  /** Whether the iframe confirmation has been accepted. */
  isIframeConfirmationAccepted,
  /** Function to accept the iframe confirmation. */
  handleIframeConfirmationAccept,
} = useVideoPlayer(props, emit)
</script>

<template>
  <div v-if="shouldRenderSurface" :class="surfaceContainerClass">
    <div
      v-if="shouldRenderSurfaceLoadingState"
      class="entry-details__player-state entry-details__player-state--loading"
      :class="surfaceStateClasses"
      :aria-label="t('media.loading')"
      role="status"
    >
      <LoadingSpinner />
    </div>

    <div
      v-else-if="shouldRenderSurfaceErrorState"
      class="entry-details__player-state"
    >
      {{ props.mediaPlayerErrorMessage }}
    </div>

    <iframe
      v-if="activeIframeSource && shouldRenderIframe"
      v-bind="standaloneRendererAttrs"
      :key="activeSurfaceRendererKey"
      :class="activeIframeClass"
      :src="activeIframeSource.src"
      :title="activeIframeTitle ?? undefined"
      :allow="activeIframeAllow ?? undefined"
      :allowfullscreen="activeIframeAllowFullscreen || undefined"
      :loading="activeIframeLoading"
      :referrerpolicy="activeIframeReferrerPolicy ?? undefined"
      :aria-hidden="activeIframeAriaHidden || undefined"
      :tabindex="activeIframeTabIndex ?? undefined"
    />

    <div
      v-if="shouldShowIframeConfirmation"
      class="entry-details__player-state entry-details__player-state--confirmation"
    >
      <img
        v-if="mediaPosterUrl"
        class="entry-details__player-confirmation-image"
        :src="mediaPosterUrl"
        :alt="displayTitle ?? undefined"
        @error="markImageUrlFailed(mediaPosterUrl)"
      />
      <button
        type="button"
        class="entry-details__player-confirmation-button"
        @click="handleIframeConfirmationAccept"
      >
        {{ t('player.loadEmbeddedPlayer') }}
      </button>
    </div>

    <div
      v-if="shouldBlockIframe"
      class="entry-details__player-state entry-details__player-state--blocked"
    >
      {{ t('player.embeddedBlocked') }}
    </div>

<VideoJsMediaRenderer
      v-if="activeVideoSource"
      :key="`video-${activeSurfaceRendererKey}`"
      :class="activeVideoClass"
      :source="(activeVideoSource as ResolvedVideoMediaSource)"
      :poster="activeVideoPoster"
      :overlay-logo-url="activeVideoOverlayLogoUrl"
      :autoplay="activeVideoAutoplay"
      :muted="activeVideoMuted"
      :loop="activeVideoLoop"
      :playsinline="activeVideoPlaysinline"
      :preload="activeVideoPreload"
      :controls="activeVideoControls"
      :aria-hidden="activeVideoAriaHidden"
      :video-aria-label="activeVideoAriaLabel"
       :tab-index="activeVideoTabIndex"
       :initial-playback-time="activeVideoInitialPlaybackTime"
       :preserve-playback-on-source-switch="activeVideoPreservePlaybackOnSourceSwitch"
       :initial-player-state="persistedVideoPlayerState"
      :show-episode-autoplay-toggle="activeVideoShowEpisodeAutoplayToggle"
      :is-episode-autoplay-enabled="activeVideoIsEpisodeAutoplayEnabled"
      :is-external-loading="shouldRenderSurfaceLoadingState"
      :show-big-play-button="activeVideoShowBigPlayButton"
      :show-video-navigation-controls="activeVideoShowVideoNavigationControls"
       :has-previous-video="activeVideoHasPreviousVideo"
       :has-next-video="activeVideoHasNextVideo"
       :previous-video-title="props.previousVideoTitle"
       :next-video-title="props.nextVideoTitle"
      v-bind="standaloneRendererAttrs"
      @update:playback-progress="handleActiveVideoPlaybackProgressUpdate"
      @update:player-state="handlePlayerStateUpdate"
      @update:is-episode-autoplay-enabled="handleActiveVideoEpisodeAutoplayEnabledUpdate"
      @playback-started="handleActiveVideoPlaybackStarted"
      @playback-ended="handleActiveVideoPlaybackEnded"
      @video-metadata-loaded="emit('video-metadata-loaded', $event)"
      @navigate-video="handleActiveVideoVideoNavigation"
      @source-error="emit('source-error')"
    />

    <div v-if="shouldShowDetailsPlayerPicker" class="entry-details__player-picker">
      <div class="entry-details__player-picker-row">
        <div
          v-if="props.showLanguageSelector"
          class="entry-details__player-picker-group"
        >
          <label class="entry-details__player-picker-label" for="entry-player-language-select">
            {{ t('entry.language') }}
          </label>

          <select
            id="entry-player-language-select"
            :value="languageModel"
            class="entry-details__player-select"
            @change="handleLanguageChange"
          >
            <option
              v-for="language in props.availableLanguages"
              :key="language.key"
              :value="language.key"
            >
              {{ language.label }}
            </option>
          </select>
        </div>

        <div
          v-if="props.showPlayerSelector"
          class="entry-details__player-picker-group"
        >
          <label class="entry-details__player-picker-label" for="entry-player-select">
            {{ t('entry.player') }}
          </label>

          <select
            id="entry-player-select"
            :value="playerModel"
            class="entry-details__player-select"
            @change="handlePlayerChange"
          >
            <option
              v-for="player in props.filteredPlayers"
              :key="player.id"
              :value="player.id"
            >
              {{ player.label }}
            </option>
          </select>
        </div>

        <div v-if="props.mediaOpenUrl && !shouldHideSourceVideoAction" class="entry-details__player-picker-group">
          <a
            :href="props.mediaOpenUrl"
            target="_blank"
            rel="noopener noreferrer"
            class="entry-details__player-btn"
            :title="t('entry.openSourceVideo')"
          >
            <v-icon icon="mdi-open-in-new" size="18" />
            <span>{{ t('entry.sourceVideo') }}</span>
          </a>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.video-player__surface--standalone {
  display: contents;
}

.video-player__iframe--standalone,
.video-player__video--standalone {
  position: fixed;
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
  will-change: transform;
}

.entry-details__trailer {
  position: relative;
  overflow: hidden;
  max-width: 880px;
  border: 0;
  border-radius: var(--radius);
  background: var(--bg-surface);
  box-shadow: var(--shadow-frame);
}

.entry-details__trailer-frame {
  display: block;
  width: 100%;
  aspect-ratio: 16 / 9;
  border: 0;
  background: var(--bg-surface);
}

.entry-details__trailer-frame--video {
  overflow: hidden;
}

.entry-details__trailer-frame--video :deep(.vjs-tech),
.entry-details__trailer-frame--video :deep(video) {
  object-fit: contain;
}

.entry-details__player-state {
  display: grid;
  place-items: center;
  min-height: 240px;
  padding: 24px;
  color: var(--text-secondary);
  text-align: center;
}

.entry-details__player-state--loading {
  gap: 20px;
}

.entry-details__player-loading-spinner {
  width: 56px;
  height: 56px;
  animation: entry-details-spinner-rotate 1.4s linear infinite;
}

.entry-details__player-loading-spinner-track {
  stroke: var(--border-color-primary);
  opacity: 0.25;
}

.entry-details__player-loading-spinner-head {
  stroke: var(--color-primary);
  stroke-dasharray: 80, 200;
  stroke-dashoffset: 0;
  animation: entry-details-spinner-dash 1.4s ease-in-out infinite;
}

@keyframes entry-details-spinner-rotate {
  100% {
    transform: rotate(360deg);
  }
}

@keyframes entry-details-spinner-dash {
  0% {
    stroke-dasharray: 1, 200;
    stroke-dashoffset: 0;
  }
  50% {
    stroke-dasharray: 90, 200;
    stroke-dashoffset: -35px;
  }
  100% {
    stroke-dasharray: 90, 200;
    stroke-dashoffset: -125px;
  }
}

.entry-details__player-state--overlay {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  aspect-ratio: 16 / 9;
  z-index: 10;
  min-height: 0;
  background: var(--bg-surface);
}

.entry-details__player-state--confirmation {
  position: relative;
  display: grid;
  place-items: center;
  min-height: 0;
  aspect-ratio: 16 / 9;
  overflow: hidden;
  background: var(--bg-surface);
}

.entry-details__player-confirmation-image {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: contain;
}

.entry-details__player-confirmation-button {
  position: relative;
  z-index: 1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: var(--control-height);
  padding: 0 24px;
  border: 1px solid var(--border-color-primary);
  border-radius: 999px;
  background: var(--bg-accent-red);
  color: var(--text-primary);
  font-size: 0.95rem;
  font-weight: 600;
  cursor: pointer;
  box-shadow: var(--box-shadow-elevated);
  transition: transform var(--duration-fast) ease, filter var(--duration-fast) ease;
}

.entry-details__player-confirmation-button:hover {
  transform: translateY(-1px);
  filter: brightness(1.06);
}

.entry-details__player-confirmation-button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.entry-details__player-state--blocked {
  aspect-ratio: 16 / 9;
  background: var(--bg-surface);
}

.entry-details__player-picker {
  display: grid;
  gap: 8px;
  padding: 3px 16px 3px;
  background: var(--bg-surface-soft);
  border-radius: var(--radius);
}

.entry-details__player-picker-row {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 28px;
  flex-wrap: wrap;
}

.entry-details__player-picker-group {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  justify-content: center;
  min-width: 0;
}

.entry-details__player-picker-label {
  color: var(--text-secondary);
  font-size: 0.82rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  white-space: nowrap;
}

.entry-details__player-select {
  width: auto;
  min-width: 180px;
  min-height: var(--control-height);
  padding: 0 14px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface-soft);
  color: var(--text-primary);
  font-size: 0.95rem;
  font-weight: 600;
  box-shadow: var(--inset-light);
  backdrop-filter: var(--backdrop-filter-medium);
}

.entry-details__player-select:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.entry-details__player-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-height: calc(var(--control-height) - 10px);
  padding: 0 20px;
  border-radius: 999px;
  background: var(--bg-accent-red);
  color: var(--text-primary);
  font-size: 0.9rem;
  font-weight: 600;
  text-decoration: none;
  box-shadow: var(--inset-light);
  transition: transform var(--duration-fast) ease, filter var(--duration-fast) ease;
}

.entry-details__player-btn:hover {
  transform: translateY(-1px);
  filter: brightness(1.06);
}

@media (max-width: 720px) {
  .entry-details__player-picker-row,
  .entry-details__player-picker-group {
    align-items: stretch;
    flex-direction: column;
  }

  .entry-details__player-select {
    width: 100%;
  }

  .entry-details__player-btn {
    width: 100%;
    justify-content: center;
  }
}
</style>
