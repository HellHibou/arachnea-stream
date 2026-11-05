<script setup lang="ts">
import { useVideoPlayer } from '@/composables/video/useVideoPlayer'
import type { VideoPlayerComponentProps, VideoPlayerComponentEmits } from '@/composables/video/playerTypes'
import VideoJsMediaRenderer from '@/components/media/VideoJsMediaRenderer.vue'
import LoadingSpinner from '@/components/icons/LoadingSpinner.vue'
import { useI18n } from '@/i18n'
import type { ResolvedVideoMediaSource } from '@/services/players'
import { markImageUrlFailed } from '@/composables/media/useFailedImageUrls'

defineOptions({ inheritAttrs: false })

/** Component props with the existing defaults applied. */
const props = withDefaults(defineProps<VideoPlayerComponentProps>(), {
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
  previousVideoItem: null,
  nextVideoItem: null,
  serviceTitle: null,
  securityMode: 'safe',
})

const emit = defineEmits<VideoPlayerComponentEmits>()
const { t } = useI18n()

/** Unified surface refs and handlers; destructuring retains each reactive ref. */
const {
  persistedVideoPlayerState,
  shouldRenderSurface,
  shouldRenderSurfaceLoadingState,
  shouldRenderSurfaceErrorState,
  surfaceContainerClass,
  surfaceStateClasses,
  activeSurfaceRendererKey,
  activeIframeSource,
  activeVideoSource,
  standaloneRendererAttrs,
  activeIframeTitle,
  activeIframeAllow,
  activeIframeAllowFullscreen,
  activeIframeLoading,
  activeIframeReferrerPolicy,
  activeIframeAriaHidden,
  activeIframeTabIndex,
  activeIframeClass,
  activeVideoAriaLabel,
  activeVideoPoster,
  activeVideoOverlayLogoUrl,
  activeVideoAutoplay,
  activeVideoMuted,
  activeVideoLoop,
  activeVideoControls,
  activeVideoPlaysinline,
  activeVideoPreload,
  activeVideoAriaHidden,
  activeVideoTabIndex,
  activeVideoInitialPlaybackTime,
  activeVideoPreservePlaybackOnSourceSwitch,
  activeVideoShowEpisodeAutoplayToggle,
  activeVideoIsEpisodeAutoplayEnabled,
  activeVideoShowBigPlayButton,
  activeVideoClass,
  shouldShowDetailsPlayerPicker,
  languageModel,
  playerModel,
  handlePlayerStateUpdate,
  handleLanguageChange,
  handlePlayerChange,
  handleActiveVideoPlaybackProgressUpdate,
  handleActiveVideoPlaybackStarted,
  handleActiveVideoPlaybackEnded,
  handleActiveVideoEpisodeAutoplayEnabledUpdate,
  activeVideoShowVideoNavigationControls,
  activeVideoHasPreviousVideo,
  activeVideoHasNextVideo,
  handleActiveVideoVideoNavigation,
  shouldShowIframeConfirmation,
  shouldBlockIframe,
  shouldHideSourceVideoAction,
  shouldRenderIframe,
  isIframeConfirmationAccepted,
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
       :previous-video-item="props.previousVideoItem"
       :next-video-item="props.nextVideoItem"
       :service-title="props.serviceTitle"
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

<style scoped src="./video-player/surface.css"></style>
<style scoped src="./video-player/states.css"></style>
<style scoped src="./video-player/picker.css"></style>
