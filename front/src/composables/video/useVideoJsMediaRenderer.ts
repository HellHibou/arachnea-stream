import videojs from 'video.js'
import 'video.js/dist/video-js.css'
import '@videojs/themes/dist/city/index.css'
import 'videojs-contrib-quality-menu/dist/videojs-contrib-quality-menu.css'
import 'videojs-contrib-eme'
import 'videojs-contrib-quality-menu'
import 'videojs-hotkeys'
import 'videojs-sprite-thumbnails'
import { computed, onBeforeUnmount, shallowRef, watch } from 'vue'

import type { ResolvedVideoMediaSource } from '@/services/players'
import type { VideoJsPlayerState } from '@/types/media'

import { PLAYBACK_SAVE_STEP_SECONDS, REMAINING_TIME_CLASS } from '@/composables/video/video-js-media-renderer/constants'
import {
  installAdaptiveControlBarMenuPositioning,
  installSeekOnClick,
  installTimerToggle,
  syncEpisodeAutoplayToggleControl,
  syncPrevNextVideoControls,
} from '@/composables/video/video-js-media-renderer/controls'
import { installStableFullscreenBridge } from '@/composables/video/video-js-media-renderer/fullscreen'
import {
  isDurationAvailable,
  isLiveStream,
  syncDurationAvailabilityState,
  updateLiveProgressBar,
} from '@/composables/video/video-js-media-renderer/live'
import {
  disableControlBarMenuHoverBehavior,
  getSelectedQualityLabel,
  resolveQualityPreferenceToken,
  selectPreferredVhsPlaylist,
  syncDisplayedQualityPreference,
} from '@/composables/video/video-js-media-renderer/quality'
import {
  applyPersistedPlayerState,
  capturePlayerState,
  getPlaybackEventSourceUrl,
  resolveRetainedQualityLabel,
} from '@/composables/video/video-js-media-renderer/state'
import type {
  UseVideoJsMediaRendererOptions,
  VideoJsMediaRendererEmits,
  VideoJsMediaRendererProps,
  VideoJsPlayer,
  VideoJsSourceInput,
  VideoJsTechHandle,
} from '@/composables/video/video-js-media-renderer/types'
import { t } from '@/i18n'

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
  /** Whether the poster overlay is currently visible. */
  const isPosterOverlayVisible = shallowRef(false)
  /** Whether the video is in initial loading state. */
  const isVideoInitialLoading = shallowRef(true)

   /** Cleanup function for pending source restore operation. */
   let pendingSourceRestoreCleanup: (() => void) | null = null
   /** Cleanup function for pending quality selector override. */
   let pendingQualitySelectorCleanup: (() => void) | null = null
   /** Preferred quality label selected by the user. */
   let preferredQualityLabel: string | null = null
   /** Whether the video initial load complete event has been emitted. */
   let hasEmittedInitialLoadComplete = false
   /** Whether the video metadata loaded event has been emitted. */
   let hasEmittedCurrentSourceMetadata = false

   /**
    * Emits the video initial load complete event once per player instance.
    */
   function emitVideoInitialLoadComplete() {
     if (!hasEmittedInitialLoadComplete) {
       hasEmittedInitialLoadComplete = true
       emit('video-initial-load-complete')
     }
   }

   /**
    * Emits the natural video dimensions once metadata is available.
    */
   function emitVideoMetadataLoaded() {
     if (hasEmittedCurrentSourceMetadata) {
       return
     }

     const element = videoElement.value
     const width = element?.videoWidth ?? 0
     const height = element?.videoHeight ?? 0

     if (width <= 0 || height <= 0) {
       return
     }

     emit('video-metadata-loaded', {
       width,
       height,
       aspectRatio: width / height,
     })
     hasEmittedCurrentSourceMetadata = true
   }

   /**
    * Indicates whether the logo overlay should stay mounted before playback starts.
    */
  const shouldRenderPosterOverlay = computed(() =>
    Boolean(props.overlayLogoUrl),
  )

  /**
   * Emits one sanitized playback position to the parent controller.
   *
   * @param value Playback position in seconds, or `null` when the stored progress should be cleared.
   */
  function emitPlaybackProgress(value: number | null) {
    const sanitizedPlaybackTime =
      typeof value === 'number' && Number.isFinite(value) && value > 0
        ? value
        : null

    emit('update:playback-progress', sanitizedPlaybackTime)
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
   * Notifies the parent controller that the autoplay preference changed from the player chrome.
   *
   * @param value Indicates whether the next episode should start automatically.
   */
  function emitEpisodeAutoplayEnabledUpdate(value: boolean) {
    emit('update:is-episode-autoplay-enabled', value)
  }

  /**
   * Emits the latest persisted Video.js UI state to the parent controller.
   *
   * @param value Captured player state, or `null` when unavailable.
   */
  function emitPlayerState(value: VideoJsPlayerState | null) {
    emit('update:player-state', value)
  }

  /**
   * Emits the latest player state when the current instance is available.
   *
   * @param player Video.js player currently bound to the renderer.
   */
  function emitCurrentPlayerState(player: VideoJsPlayer) {
    emitPlayerState(capturePlayerState(player, preferredQualityLabel))
  }

  /**
   * Removes the transient source-switch listeners attached for one in-flight media transition.
   */
  function clearPendingSourceRestoreCleanup() {
    pendingSourceRestoreCleanup?.()
    pendingSourceRestoreCleanup = null
  }

  /**
   * Removes the temporary VHS selector override attached for the active source.
   */
  function clearPendingQualitySelectorCleanup() {
    pendingQualitySelectorCleanup?.()
    pendingQualitySelectorCleanup = null
  }

   /**
    * Disposes the current Video.js instance before the renderer switches source or unmounts.
    */
   function destroyActivePlayer() {
     clearPendingSourceRestoreCleanup()
     clearPendingQualitySelectorCleanup()

     if (activePlayer.value) {
       emitCurrentPlayerState(activePlayer.value)
     }

     activePlayer.value?.dispose()
     activePlayer.value = null
     isPosterOverlayVisible.value = false
     hasEmittedInitialLoadComplete = false
   }

  /**
   * Applies the reactive accessibility and autoplay attributes directly to the video element.
   *
   * @param element Native video element managed by Video.js.
   */
  function syncVideoElementAttributes(element: HTMLVideoElement) {
    element.style.setProperty('--vjs-theme-city--primary', '#46a3ff')
    element.style.setProperty('--vjs-theme-city--secondary', '#f7fbff')
    element.defaultMuted = props.muted ?? false
    element.muted = props.muted ?? false
    element.autoplay = props.autoplay ?? false
    element.loop = props.loop ?? false
    element.playsInline = props.playsinline ?? false
    element.preload = props.preload ?? 'metadata'

    if (props.poster) {
      element.poster = props.poster
    } else {
      element.removeAttribute('poster')
    }

    if (props.ariaHidden) {
      element.setAttribute('aria-hidden', 'true')
    } else {
      element.removeAttribute('aria-hidden')
    }

    if (props.videoAriaLabel) {
      element.setAttribute('aria-label', props.videoAriaLabel)
    } else {
      element.removeAttribute('aria-label')
    }

    if (props.tabIndex === null || props.tabIndex === undefined) {
      element.removeAttribute('tabindex')
    } else {
      element.tabIndex = props.tabIndex
    }
  }

  /**
   * Builds the Video.js source descriptor expected by the EME-enabled player.
   *
   * @param source Resolved source selected for playback.
   * @returns Video.js source configuration.
   */
  function buildPlayerSource(source: ResolvedVideoMediaSource): VideoJsSourceInput {
    const spriteThumbnails = source.vttUrl
      ? { url: source.vttUrl }
      : source.storyboard
      ? { ...source.storyboard }
      : {}

    const sourceInput: VideoJsSourceInput = {
      src: source.src,
      spriteThumbnails,
    }

    if (source.mimeType) {
      sourceInput.type = source.mimeType
    }

    if (source.transport === 'dash' && source.licenseUrl) {
      sourceInput.keySystems = {
        'com.widevine.alpha': {
          licenseUri: source.licenseUrl,
          licenseHeaders: source.licenseHeaders,
        },
      }
    }

    return sourceInput
  }

   /**
    * Updates the control bar with the current autoplay toggle state.
    *
    * @param player Video.js player currently bound to the renderer.
    */
   function syncEpisodeAutoplayControl(player: VideoJsPlayer) {
     syncEpisodeAutoplayToggleControl(player, {
       controls: props.controls ?? false,
       showEpisodeAutoplayToggle: props.showEpisodeAutoplayToggle ?? false,
       isEpisodeAutoplayEnabled: props.isEpisodeAutoplayEnabled ?? false,
       onEpisodeAutoplayToggle: () => {
         emitEpisodeAutoplayEnabledUpdate(!(props.isEpisodeAutoplayEnabled ?? false))
       },
     })
   }

   /**
    * Updates the control bar with prev/next video navigation controls.
    *
    * @param player Video.js player currently bound to the renderer.
    */
   function syncPrevNextVideoControl(player: VideoJsPlayer) {
     syncPrevNextVideoControls(player, {
       controls: props.controls ?? false,
       showPrevVideoControl: props.showVideoNavigationControls ?? false,
       showNextVideoControl: props.showVideoNavigationControls ?? false,
       hasPreviousVideo: props.hasPreviousVideo ?? false,
       hasNextVideo: props.hasNextVideo ?? false,
       onPrevVideo: () => {
         emit('navigate-video', -1)
       },
       onNextVideo: () => {
         emit('navigate-video', 1)
       },
     })
   }

   /**
    * Applies the host attribute used to suppress the big play button without waiting for a Vue render.
    *
    * @param shouldSuppressBigPlayButton Indicates whether the big play overlay should stay hidden.
    */
   function syncBigPlaySuppressionAttribute(shouldSuppressBigPlayButton: boolean) {
     const element = hostElement.value

     if (!element) {
       return
     }

     if (shouldSuppressBigPlayButton) {
       element.setAttribute('data-vjs-initial-loading', 'true')
       return
     }

     element.removeAttribute('data-vjs-initial-loading')
   }

   /**
    * Marks the video as being in its initial loading phase.
    */
   function markVideoInitialLoadStart() {
     isVideoInitialLoading.value = true
     hasEmittedCurrentSourceMetadata = false
     syncBigPlaySuppressionAttribute(true)
   }

   /**
    * Marks the video as having finished initial loading.
    */
   function markVideoInitialLoadComplete() {
     isVideoInitialLoading.value = false
     syncBigPlaySuppressionAttribute(props.isExternalLoading ?? false)
   }

  /**
   * Applies the reactive configuration that can change without recreating the player.
   *
   * @param player Video.js player currently bound to the renderer.
   */
  function syncExistingPlayerConfiguration(player: VideoJsPlayer) {
    const element = videoElement.value

    if (element) {
      syncVideoElementAttributes(element)
    }

    player.poster(props.poster ?? '')
    player.controls(props.controls ?? false)
    player.loop(props.loop ?? false)
    syncEpisodeAutoplayControl(player)
    syncPrevNextVideoControl(player)
  }

  /**
   * Starts playback when autoplay is enabled and the current source is ready enough.
   *
   * @param player Video.js player currently bound to the renderer.
   */
  function attemptAutoplay(player: VideoJsPlayer) {
    if (!props.autoplay) {
      return
    }

    const playPromise = player.play()

    if (!playPromise) {
      return
    }

    void playPromise.catch(() => {})
  }

  /**
   * Pins the VHS playlist selector to the preferred quality bucket for the current source.
   *
   * @param player Video.js player currently bound to the renderer.
   * @param qualityLabel Preferred quality label captured from the previous video state.
   * @param expectedSourceUrl Source URL that should receive the selector override.
   */
  function configureQualityPreferenceSelector(
    player: VideoJsPlayer,
    qualityLabel: string | null,
    expectedSourceUrl: string,
  ) {
    clearPendingQualitySelectorCleanup()

    const qualityPreferenceToken = resolveQualityPreferenceToken(qualityLabel)

    if (!qualityPreferenceToken) {
      return
    }

    const applySelectorOverride = () => {
      if (activePlayer.value !== player || props.source.src !== expectedSourceUrl) {
        return false
      }

      const tech = player.tech(true) as VideoJsTechHandle | null
      const vhs = tech?.vhs

      if (!vhs) {
        return false
      }

      const fallbackSelectPlaylist = vhs.selectPlaylist.bind(vhs)
      vhs.selectPlaylist = function selectPreferredPlaylistWrapper() {
        return selectPreferredVhsPlaylist(
          this as typeof vhs,
          qualityPreferenceToken,
        ) ?? fallbackSelectPlaylist()
      }

      pendingQualitySelectorCleanup = () => {
        const currentTech = player.tech(true) as VideoJsTechHandle | null

        if (currentTech?.vhs === vhs) {
          vhs.selectPlaylist = fallbackSelectPlaylist
        }

        pendingQualitySelectorCleanup = null
      }

      return true
    }

    if (applySelectorOverride()) {
      return
    }

    const retryTimer = window.setTimeout(() => {
      applySelectorOverride()
    }, 0)

    pendingQualitySelectorCleanup = () => {
      window.clearTimeout(retryTimer)
      pendingQualitySelectorCleanup = null
    }
  }

  /**
   * Restores one persisted player state after a source switch or component remount.
   *
   * @param player Video.js player currently bound to the renderer.
   * @param state Persisted player state that should be restored.
   */
  function restorePlayerState(player: VideoJsPlayer, state: VideoJsPlayerState | null) {
    applyPersistedPlayerState(player, state)
  }

   /**
    * Applies one new media source to the current player while preserving the user state.
    *
    * @param player Video.js player currently bound to the renderer.
    * @param source Resolved source selected for playback.
    * @param playerState State snapshot that should survive the source switch.
    */
   function applySourceToPlayer(
     player: VideoJsPlayer,
     source: ResolvedVideoMediaSource,
     playerState: VideoJsPlayerState | null,
   ) {
     clearPendingSourceRestoreCleanup()
     clearPendingQualitySelectorCleanup()
     markVideoInitialLoadStart()
     isPosterOverlayVisible.value = shouldRenderPosterOverlay.value && !props.autoplay
     let retainedQualityLabel = resolveRetainedQualityLabel(preferredQualityLabel, playerState)

    let playbackRestored = false
    let playerStateRestored = false

    const restorePlaybackTime = () => {
      if (playbackRestored) {
        return
      }

      if (
        typeof props.initialPlaybackTime !== 'number' ||
        !Number.isFinite(props.initialPlaybackTime) ||
        props.initialPlaybackTime <= 0
      ) {
        return
      }

      const duration = player.duration()
      const safePlaybackTime =
        typeof duration === 'number' && Number.isFinite(duration) && duration > 0
          ? Math.min(props.initialPlaybackTime, Math.max(duration - 2, 0))
          : props.initialPlaybackTime

      if (safePlaybackTime <= 0) {
        return
      }

      player.currentTime(safePlaybackTime)
      playbackRestored = true
    }

    const restorePlayerUiState = () => {
      if (playerStateRestored) {
        return
      }

      restorePlayerState(player, playerState)
      playerStateRestored = true
    }

    const syncRetainedQualityPreference = (treatMissingMenuAsUnavailable = false) => {
      const isQualityPreferenceAvailable = syncDisplayedQualityPreference(
        player,
        retainedQualityLabel,
        { treatMissingMenuAsUnavailable },
      )

      if (isQualityPreferenceAvailable) {
        return
      }

      retainedQualityLabel = null
      preferredQualityLabel = null
      clearPendingQualitySelectorCleanup()
      emitCurrentPlayerState(player)
    }

     const handleLoadedMetadata = () => {
       emitVideoMetadataLoaded()
       disableControlBarMenuHoverBehavior(player)
       installSeekOnClick(player, {
         controls: props.controls ?? false,
       })
       restorePlaybackTime()
       restorePlayerUiState()
       syncRetainedQualityPreference()

       if (player.readyState() >= 3) {
         markVideoInitialLoadComplete()
         emitVideoInitialLoadComplete()
         attemptAutoplay(player)
       }
     }

     const handleCanPlay = () => {
       emitVideoMetadataLoaded()
       restorePlaybackTime()
       restorePlayerUiState()
       syncRetainedQualityPreference(true)
       markVideoInitialLoadComplete()
       emitVideoInitialLoadComplete()
       attemptAutoplay(player)
     }

     const handlePlaying = () => {
       markVideoInitialLoadComplete()
       emitVideoInitialLoadComplete()
       clearPendingSourceRestoreCleanup()
     }

    syncExistingPlayerConfiguration(player)
    player.on('loadedmetadata', handleLoadedMetadata)
    player.on('canplay', handleCanPlay)
    player.on('playing', handlePlaying)
    pendingSourceRestoreCleanup = () => {
      player.off('loadedmetadata', handleLoadedMetadata)
      player.off('canplay', handleCanPlay)
      player.off('playing', handlePlaying)
    }
    player.src(buildPlayerSource(source))
    configureQualityPreferenceSelector(player, retainedQualityLabel, source.src)
  }

  /**
   * Creates one fresh Video.js player instance for the current element and source.
   *
   * @param element Native video element enhanced by Video.js.
   * @param source Resolved source selected for playback.
   */
   function createPlayer(element: HTMLVideoElement, source: ResolvedVideoMediaSource) {
     syncVideoElementAttributes(element)
     isPosterOverlayVisible.value = shouldRenderPosterOverlay.value && !props.autoplay
     markVideoInitialLoadStart()

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
       notSupportedMessage: t('errors.videoNotSupported'),
     }) as VideoJsPlayer

     activePlayer.value = player
     installStableFullscreenBridge(player, () => hostElement.value)

     player.on('error', () => {
       console.error('[Video.js] Playback error:', player.error())
       emit('source-error')
       markVideoInitialLoadComplete()
       emitVideoInitialLoadComplete()
     })

     player.ready(() => {
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

        if (props.controls && typeof player.spriteThumbnails === 'function') {
          player.spriteThumbnails(source.vttUrl
            ? { url: source.vttUrl }
            : (source.storyboard ? { ...source.storyboard } : {}))
        }

       /** Last playback step that was saved to persist progress. */
       let lastSavedPlaybackStep = -1

       /**
        * Handles click events on the quality menu.
        * @param event - DOM click event.
        */
       const handleQualityMenuClick = (event: Event) => {
        const target = event.target as HTMLElement | null

        if (!target?.closest('.vjs-quality-menu-wrapper .vjs-menu-item')) {
          return
        }

        window.setTimeout(() => {
          preferredQualityLabel = getSelectedQualityLabel(player)
          emitCurrentPlayerState(player)
        }, 0)
      }

      playerElement?.addEventListener('click', handleQualityMenuClick)

      /**
       * Emits the current player state after UI updates have settled.
       */
      const emitPlayerStateAfterUiUpdate = () => {
        window.setTimeout(() => {
          emitCurrentPlayerState(player)
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
      syncEpisodeAutoplayControl(player)
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
        emitPlaybackProgress(currentTime)
      })

      player.on('pause', () => {
        const currentTime = player.currentTime()
        emitPlaybackProgress(typeof currentTime === 'number' ? currentTime : null)
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

      /**
       * Hides the poster overlay from the video player.
       */
      const hidePosterOverlay = () => {
        isPosterOverlayVisible.value = false
      }

      player.on('play', hidePosterOverlay)
      player.on('playing', () => {
        hidePosterOverlay()
        emitPlaybackStarted(getPlaybackEventSourceUrl(player, props.source.src))
      })

      player.on('ended', () => {
        lastSavedPlaybackStep = -1
        emitPlaybackProgress(null)
        emitPlaybackEnded()
      })

      player.on('dispose', () => {
        playerElement?.removeEventListener('click', handleQualityMenuClick)
        audioTracks?.removeEventListener?.('change', emitPlayerStateAfterUiUpdate)
        textTracks?.removeEventListener?.('change', emitPlayerStateAfterUiUpdate)
        playerElement?.removeEventListener('change', handleTextTrackSettingsChange, true)
        playerElement?.removeEventListener('click', handleTextTrackSettingsClick, true)
      })

      applySourceToPlayer(player, source, props.initialPlayerState ?? null)
    })
  }

  watch(
    () => videoElement.value,
    (element) => {
      destroyActivePlayer()

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
      if (!activePlayer.value || !previousSource || previousSource.src === nextSource.src) {
        return
      }

      const playerState = capturePlayerState(activePlayer.value, preferredQualityLabel)
      emitPlayerState(playerState)
      applySourceToPlayer(activePlayer.value, nextSource, playerState)
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
    (isExternalLoading) => {
      syncBigPlaySuppressionAttribute(isVideoInitialLoading.value || (isExternalLoading ?? false))
    },
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

      syncEpisodeAutoplayControl(activePlayer.value)
    },
  )

  watch(
    [
      () => props.showVideoNavigationControls,
      () => props.hasPreviousVideo,
      () => props.hasNextVideo,
    ],
    () => {
      if (!activePlayer.value) {
        return
      }

      syncPrevNextVideoControl(activePlayer.value)
    },
  )

   onBeforeUnmount(() => {
     destroyActivePlayer()
   })

   return {
     isPosterOverlayVisible,
     shouldRenderPosterOverlay,
     isVideoInitialLoading,
   }
}
