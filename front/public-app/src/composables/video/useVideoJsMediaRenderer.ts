import videojs from 'video.js'
import 'video.js/dist/video-js.css'
import '@videojs/themes/dist/city/index.css'
import 'videojs-contrib-quality-menu/dist/videojs-contrib-quality-menu.css'
import 'videojs-contrib-eme'
import 'videojs-contrib-quality-menu'
import 'videojs-hotkeys'
import 'videojs-sprite-thumbnails'
import { onBeforeUnmount, shallowRef, watch } from 'vue'

import type { ResolvedVideoMediaSource } from '@/services/players'
import { useStorage } from '@/services/storage'
import { t } from '@/i18n'
import { markImageUrlFailed } from '@/composables/media/useFailedImageUrls'
import { PLAYBACK_SAVE_STEP_SECONDS, REMAINING_TIME_CLASS } from './video-js-media-renderer/constants'
import {
  installAdaptiveControlBarMenuPositioning, installTimerToggle, syncPrevNextVideoControls,
} from './video-js-media-renderer/controls'
import { installStableFullscreenBridge } from './video-js-media-renderer/fullscreen'
import {
  installAutoSkipChapters, installChapterOverlay, installChapterSegments,
  installSkipChapterButton, SKIP_CHAPTER_BUTTON_TYPES, type SkipChapterType,
} from './video-js-media-renderer/chapters'
import { installDashPeriodChapters, isDashSource } from './video-js-media-renderer/dash'
import {
  isDurationAvailable, isLiveStream, syncDurationAvailabilityState, updateLiveProgressBar,
} from './video-js-media-renderer/live'
import { disableControlBarMenuHoverBehavior } from './video-js-media-renderer/quality'
import { getPlaybackEventSourceUrl } from './video-js-media-renderer/state'
import type { UseVideoJsMediaRendererOptions, VideoJsPlayer } from './video-js-media-renderer/types'
import { areSubtitlesEqual } from './video-js-media-renderer/subtitles'
import { syncVideoElementAttributes, installPosterImageErrorHandler } from './video-js-media-renderer/playerAttributes'
import { syncEpisodeAutoplayControl } from './video-js-media-renderer/autoplayMenu'
import { createQualityController } from './video-js-media-renderer/qualityController'
import { createStoryboardController } from './video-js-media-renderer/storyboardController'
import { createSourceController } from './video-js-media-renderer/sourceController'
import { createPlayerUiState } from './video-js-media-renderer/playerUiState'
import type { VideoJsTrackSelectionConfig } from './video-js-media-renderer/trackPreferences'

export type {
  VideoJsMediaDimensions,
  VideoJsMediaRendererEmits,
  VideoJsMediaRendererProps,
} from '@/composables/video/video-js-media-renderer/types'

export type { VideoJsSourceInput } from '@/composables/video/video-js-media-renderer/types'

/**
 * Manages the lifecycle and UI integration of one Video.js renderer instance.
 *
 * @param options Component props, emits, and template refs required to control the player.
 * @returns Reactive view state consumed by the renderer component.
 */
export function useVideoJsMediaRenderer(options: UseVideoJsMediaRendererOptions) {
  const { props, emit, hostElement, videoElement } = options
  /** Current active Video.js player instance. */
  const activePlayer = shallowRef<VideoJsPlayer | null>(null)
  const ui = createPlayerUiState(options)

  /** Builds the track selection configuration from current storage parameters. */
  function buildTrackSelectionConfig(): VideoJsTrackSelectionConfig {
    const parameters = useStorage().getParameters()
    return {
      profiles: parameters['videoPlayer.trackProfiles'].value,
      subtitleLanguagePriority: parameters['videoPlayer.subtitleLanguagePriority'].value,
      allowAudioDescription: parameters['videoPlayer.allowAudioDescription'].value,
    }
  }

  function getTrackSelectionConfig(): VideoJsTrackSelectionConfig {
    return buildTrackSelectionConfig()
  }

  const quality = createQualityController({
    getPlayer: () => activePlayer.value,
    getSourceUrl: () => props.source.src,
    onStateChange: emitCurrentPlayerState,
    getTrackSelectionConfig,
  })
  const storyboard = createStoryboardController({
    getPlayer: () => activePlayer.value,
    getSource: () => props.source,
    hasControls: () => props.controls ?? false,
  })
  const sources = createSourceController({
    props, getPlayer: () => activePlayer.value,
    configurePlayer: syncExistingPlayerConfiguration, quality, storyboard, ui,
    getTrackSelectionConfig,
  })

  /**
   * Persists player UI state using the quality controller's current preference.
   * @param player Player whose state is being saved.
   */
  function emitCurrentPlayerState(player: VideoJsPlayer) {
    emit('update:player-state', quality.captureState(player))
  }

  /**
   * Emits one sanitized playback position and duration to the parent controller.
   *
   * @param value Playback position in seconds, or `null` when the stored progress should be cleared.
   * @param duration Total media duration in seconds, or `null` when unavailable.
   */
  function emitPlaybackProgress(value: number | null, duration: number | null = null) {
    const sanitizedPlaybackTime =
      typeof value === 'number' && Number.isFinite(value) && value > 0
        ? value
        : null
    const sanitizedDuration =
      typeof duration === 'number' && Number.isFinite(duration) && duration > 0
        ? duration
        : null

    emit('update:playback-progress', sanitizedPlaybackTime, sanitizedDuration)
  }
  /**
   * Notifies the parent controller that playback has effectively started.
   *
   * @param sourceUrl Active source URL reported by the player.
   */
  function emitPlaybackStarted(sourceUrl: string | null) {
    emit('playback-started', sourceUrl)
  }
  /**
   * Notifies the parent controller that playback reached the end of the current source.
   */
  function emitPlaybackEnded() {
    emit('playback-ended')
  }
  /**
   * Applies the reactive accessibility and autoplay attributes directly to the video element.
   *
   * @param player Player receiving the reactive configuration.
   */
  function syncExistingPlayerConfiguration(player: VideoJsPlayer) {
    const element = videoElement.value

    if (element) {
      syncVideoElementAttributes(element, props)
    }
    player.poster(props.poster ?? '')
    installPosterImageErrorHandler(player, props.poster ?? undefined, markImageUrlFailed)
    player.controls(props.controls ?? false)
    player.loop(props.loop ?? false)
    syncEpisodeAutoplayControl(player, props, emit, activePlayer)
    syncPrevNextVideoControls(player, {
      controls: props.controls ?? false,
      showPrevVideoControl: props.showVideoNavigationControls ?? false,
      showNextVideoControl: props.showVideoNavigationControls ?? false,
      previousVideoTitle: props.previousVideoTitle ?? null,
      nextVideoTitle: props.nextVideoTitle ?? null,
      hasPreviousVideo: props.hasPreviousVideo ?? false,
      hasNextVideo: props.hasNextVideo ?? false,
      suppressPreviousVideoTitle: Boolean(props.previousVideoItem),
      suppressNextVideoTitle: Boolean(props.nextVideoItem),
      onPrevVideo: () => {
        emit('navigate-video', -1)
      },
      onNextVideo: () => {
        emit('navigate-video', 1)
      },
      onPrevVideoPreviewChange: (isPreviewed) => {
        ui.previewNavigation(isPreviewed ? -1 : null)
      },
      onNextVideoPreviewChange: (isPreviewed) => {
        ui.previewNavigation(isPreviewed ? 1 : null)
      },
    })
  }
  /** Saves state, invalidates pending transitions and disposes the current player. */
  function destroyCurrentPlayer() {
    const player = activePlayer.value
    if (player) emitCurrentPlayerState(player)
    sources.reset()
    player?.dispose()
    activePlayer.value = null
    storyboard.dispose()
    ui.reset()
  }

  /**
   * Creates the player and attaches instance-scoped integrations once it is ready.
   * @param element Native video element managed by Video.js.
   * @param source Initial source used to install chapter controls.
   */
  function createPlayer(element: HTMLVideoElement, source: ResolvedVideoMediaSource) {
    syncVideoElementAttributes(element, props)
    ui.startSource()

    const player = videojs(element, {
      autoplay: props.autoplay ?? false,
      controls: props.controls ?? false,
      loop: props.loop ?? false,
      muted: props.muted ?? false,
      preload: props.preload ?? 'metadata',
      responsive: true,
      fluid: false,
      bigPlayButton: props.showBigPlayButton ?? (props.controls ?? false),
      controlBar: props.controls ?? false,
      inactivityTimeout: props.controls ? 2000 : 0,
      enableDocumentPictureInPicture: props.controls ?? false,
      liveui: true,
      // Required by the DASH source handler so dash.js owns text track rendering.
      html5: {
        nativeCaptions: false,
      },
      notSupportedMessage: t('errors.videoNotSupported'),
    }) as VideoJsPlayer

    activePlayer.value = player
    installStableFullscreenBridge(player, () => hostElement.value)

    player.on('error', () => {
      console.error('[Video.js] Playback error:', player.error())
      emit('source-error')
      ui.complete()
    })

    player.ready(() => {
      if (activePlayer.value !== player) {
        return
      }
      const playerElement = player.el()

      // Initialize plugins after player is fully ready
      if (typeof player.eme === 'function') {
        player.eme()
      }
      if (typeof player.qualityLevels === 'function') {
        player.qualityLevels()
      }
      if (props.controls && typeof player.qualityMenu === 'function') {
        player.qualityMenu({
          defaultResolution: 'none',
          useResolutionLabels: true,
        })
      }
      if (props.controls && typeof player.hotkeys === 'function') {
        player.hotkeys({
          enableModifiersForNumbers: false,
          enableVolumeScroll: false,
          seekStep: 5,
          volumeStep: 0.1,
        })
      }
      storyboard.attach(player)

      if (props.controls) {
        const baseChapters = source.chapters ?? []
        const isAutoskipEnabled = (chapterType: SkipChapterType): boolean => {
          if (!useStorage().getParameters()[`videoPlayer.autoskip.${chapterType}`].value) {
            return false
          }
          // Advertising is skipped unconditionally; other chapter types are
          // only skipped automatically while episode autoplay is enabled.
          return chapterType === 'ads' || (props.isEpisodeAutoplayEnabled ?? false)
        }
        const dashChaptersHandled = isDashSource(source) &&
          installDashPeriodChapters(player, baseChapters, { isAutoskipEnabled })

        if (!dashChaptersHandled && baseChapters.length > 0) {
          installChapterOverlay(player, baseChapters)
          installChapterSegments(player, baseChapters)
        }
        if (baseChapters.length > 0) {
          for (const chapterType of SKIP_CHAPTER_BUTTON_TYPES) {
            if (dashChaptersHandled && chapterType === 'ads') {
              continue
            }
            installSkipChapterButton(player, baseChapters, chapterType)
          }
          if (!dashChaptersHandled) {
            installAutoSkipChapters(player, { chapters: baseChapters, isAutoskipEnabled })
          }
        }
      }
      /** Last playback step that was saved to persist progress. */
      let lastSavedPlaybackStep = -1

      quality.attach(player)

      /**
       * Emits the current player state after UI updates have settled.
       */
      const emitPlayerStateAfterUiUpdate = () => {
        window.setTimeout(() => {
          if (activePlayer.value === player) emitCurrentPlayerState(player)
        }, 0)
      }
      /**
       * Handles change events on text track settings.
       * @param event - DOM change event.
       */
      const handleTextTrackSettingsChange = (event: Event) => {
        const target = event.target as HTMLElement | null

        if (!target?.closest('.vjs-text-track-settings')) {
          return
        }
        emitPlayerStateAfterUiUpdate()
      }
      /**
       * Handles click events on text track settings button.
       * @param event - DOM click event.
       */
      const handleTextTrackSettingsClick = (event: Event) => {
        const target = event.target as HTMLElement | null

        if (!target?.closest('.vjs-text-track-settings .vjs-default-button')) {
          return
        }
        emitPlayerStateAfterUiUpdate()
      }
      const audioTracks = player.audioTracks?.()
      const textTracks = player.textTracks?.()

      audioTracks?.addEventListener?.('change', emitPlayerStateAfterUiUpdate)
      textTracks?.addEventListener?.('change', emitPlayerStateAfterUiUpdate)
      playerElement?.addEventListener('change', handleTextTrackSettingsChange, true)
      playerElement?.addEventListener('click', handleTextTrackSettingsClick, true)

      installTimerToggle(player, {
        controls: props.controls ?? false,
        onStateChange: () => {
          emitCurrentPlayerState(player)
        },
      })
      syncDurationAvailabilityState(player)
      syncEpisodeAutoplayControl(player, props, emit, activePlayer)
      disableControlBarMenuHoverBehavior(player)
      installAdaptiveControlBarMenuPositioning(player)

      /**
       * Handles duration change events on the player.
       */
      const handleDurationChange = () => {
        syncDurationAvailabilityState(player)

        if (!isDurationAvailable(player)) {
          player.el()?.classList.remove(REMAINING_TIME_CLASS)
        }
        if (!isLiveStream(player) || !player.liveTracker) {
          return
        }
        player.liveTracker.seekable = () => {
          const seekable = player.seekable()
          return seekable && seekable.length > 0 ? seekable : null
        }
        player.liveTracker.liveCurrentTime = () => {
          return player.currentTime() || 0
        }
        updateLiveProgressBar(player)
      }
      player.on('durationchange', handleDurationChange)
      handleDurationChange()

      player.on('timeupdate', () => {
        const currentTime = player.currentTime()

        if (typeof currentTime !== 'number' || !Number.isFinite(currentTime) || currentTime <= 0) {
          return
        }
        if (isLiveStream(player)) {
          updateLiveProgressBar(player)
        }
        const playbackStep = Math.floor(currentTime / PLAYBACK_SAVE_STEP_SECONDS)

        if (playbackStep === lastSavedPlaybackStep) {
          return
        }
        lastSavedPlaybackStep = playbackStep
        emitPlaybackProgress(currentTime, player.duration() ?? null)
      })

      player.on('pause', () => {
        const currentTime = player.currentTime()
        emitPlaybackProgress(typeof currentTime === 'number' ? currentTime : null, player.duration() ?? null)
      })

      player.on('volumechange', () => {
        emitCurrentPlayerState(player)
      })

      player.on('progress', () => {
        if (isLiveStream(player)) {
          updateLiveProgressBar(player)
        }
      })

      player.on('seeked', () => {
        if (isLiveStream(player)) {
          updateLiveProgressBar(player)
        }
      })

      player.on('play', ui.hidePoster)
      player.on('playing', () => {
        ui.hidePoster()
        emitPlaybackStarted(getPlaybackEventSourceUrl(player, props.source.src))
      })

      player.on('ended', () => {
        lastSavedPlaybackStep = -1
        emitPlaybackProgress(null)
        emitPlaybackEnded()
      })

      player.on('dispose', () => {
        audioTracks?.removeEventListener?.('change', emitPlayerStateAfterUiUpdate)
        textTracks?.removeEventListener?.('change', emitPlayerStateAfterUiUpdate)
        playerElement?.removeEventListener('change', handleTextTrackSettingsChange, true)
        playerElement?.removeEventListener('click', handleTextTrackSettingsClick, true)
      })

      void sources.applySource(player, props.source, { playerState: props.initialPlayerState ?? null })
    })
  }
  watch(
    () => videoElement.value,
    (element) => {
      destroyCurrentPlayer()

      if (!element) {
        return
      }
      createPlayer(element, props.source)
    },
    { immediate: true },
  )

  watch(
    () => props.source,
    (nextSource, previousSource) => {
      if (
        !activePlayer.value ||
        !previousSource ||
        (previousSource.src === nextSource.src && areSubtitlesEqual(previousSource.subtitles, nextSource.subtitles))
      ) {
        return
      }
      const player = activePlayer.value
      const playerState = quality.captureState(player)
      emit('update:player-state', playerState)

      const currentTime = player.currentTime()
      const pendingTime = props.preservePlaybackOnSourceSwitch &&
        typeof currentTime === 'number' && Number.isFinite(currentTime) && currentTime > 0
        ? currentTime
        : null
      const pendingAutoplay = props.preservePlaybackOnSourceSwitch &&
        typeof player.paused === 'function' && !player.paused()

      void sources.applySource(player, nextSource, {
        playerState, switchTime: pendingTime, switchAutoplay: pendingAutoplay,
      })
    },
  )

  watch(
    [
      () => props.poster,
      () => props.overlayLogoUrl,
      () => props.muted,
      () => props.loop,
      () => props.controls,
      () => props.playsinline,
      () => props.preload,
      () => props.ariaHidden,
      () => props.videoAriaLabel,
      () => props.tabIndex,
    ],
    () => {
      if (!activePlayer.value) {
        return
      }
      syncExistingPlayerConfiguration(activePlayer.value)
    },
  )

  watch(
    () => props.autoplay,
    (nextAutoplay) => {
      if (videoElement.value) {
        videoElement.value.autoplay = nextAutoplay ?? false
      }
      activePlayer.value?.autoplay(nextAutoplay ?? false)
    },
  )

  watch(
    () => props.isExternalLoading,
    ui.syncLoading,
  )

  watch(
    [
      () => props.showEpisodeAutoplayToggle,
      () => props.isEpisodeAutoplayEnabled,
    ],
    () => {
      if (!activePlayer.value) {
        return
      }
      syncEpisodeAutoplayControl(activePlayer.value, props, emit, activePlayer)
    },
  )

  watch(
    [
      () => props.showVideoNavigationControls,
      () => props.hasPreviousVideo,
      () => props.hasNextVideo,
      () => props.previousVideoTitle,
      () => props.nextVideoTitle,
      () => props.previousVideoItem,
      () => props.nextVideoItem,
    ],
    () => {
      if (!activePlayer.value) {
        return
      }
      syncPrevNextVideoControls(activePlayer.value, {
        controls: props.controls ?? false,
        showPrevVideoControl: props.showVideoNavigationControls ?? false,
        showNextVideoControl: props.showVideoNavigationControls ?? false,
        previousVideoTitle: props.previousVideoTitle ?? null,
        nextVideoTitle: props.nextVideoTitle ?? null,
        hasPreviousVideo: props.hasPreviousVideo ?? false,
        hasNextVideo: props.hasNextVideo ?? false,
        suppressPreviousVideoTitle: Boolean(props.previousVideoItem),
        suppressNextVideoTitle: Boolean(props.nextVideoItem),
        onPrevVideo: () => {
          emit('navigate-video', -1)
        },
        onNextVideo: () => {
          emit('navigate-video', 1)
        },
        onPrevVideoPreviewChange: (isPreviewed) => {
          ui.previewNavigation(isPreviewed ? -1 : null)
        },
        onNextVideoPreviewChange: (isPreviewed) => {
          ui.previewNavigation(isPreviewed ? 1 : null)
        },
      })
    },
  )

  onBeforeUnmount(() => {
    destroyCurrentPlayer()
  })

  return {
    isPosterOverlayVisible: ui.isPosterOverlayVisible,
    shouldRenderPosterOverlay: ui.shouldRenderPosterOverlay,
    isVideoInitialLoading: ui.isVideoInitialLoading,
    storyboardPreviewSize: storyboard.storyboardPreviewSize,
    vttStoryboardGrid: storyboard.vttStoryboardGrid,
    previewedNavigationDirection: ui.previewedNavigationDirection,
  }
}
