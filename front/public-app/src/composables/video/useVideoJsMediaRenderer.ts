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
import { useStorage } from '@/services/storage'
import type { VideoJsPlayerState } from '@/types/media'

import { PLAYBACK_SAVE_STEP_SECONDS, REMAINING_TIME_CLASS } from '@/composables/video/video-js-media-renderer/constants'
import {
  installAdaptiveControlBarMenuPositioning,
  installTimerToggle,
  syncPrevNextVideoControls,
} from '@/composables/video/video-js-media-renderer/controls'
import { installStableFullscreenBridge } from '@/composables/video/video-js-media-renderer/fullscreen'
import {
  installAutoSkipChapters,
  installChapterOverlay,
  installChapterSegments,
  installSkipChapterButton,
  SKIP_CHAPTER_BUTTON_TYPES,
  type SkipChapterType,
} from '@/composables/video/video-js-media-renderer/chapters'
import {
  installDashPeriodChapters,
  installDashQualityBridge,
  isDashSource,
  type DashQualityBridge,
} from '@/composables/video/video-js-media-renderer/dash'
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
} from '@/composables/video/video-js-media-renderer/quality'
import {
  capturePlayerState,
  getPlaybackEventSourceUrl,
  restorePersistedTrackPreferences,
} from '@/composables/video/video-js-media-renderer/state'
import type {
  UseVideoJsMediaRendererOptions,
  VideoJsPlayer,
  VideoJsRemoteTextTrackHandle,
  VideoJsSpriteThumbnailsPlugin,
  VideoJsSourceInput,
  VideoJsTechHandle,
} from '@/composables/video/video-js-media-renderer/types'
import { t } from '@/i18n'
import { markImageUrlFailed } from '@/composables/media/useFailedImageUrls'

import {
  STORYBOARD_PREVIEW_WIDTH,
  STORYBOARD_PREVIEW_HEIGHT,
  STORYBOARD_RESPONSIVE_WIDTH,
  type StoryboardVttCue,
  installStoryboardVttCueOverride,
} from '@/composables/video/video-js-media-renderer/storyboard'
import { areSubtitlesEqual } from '@/composables/video/video-js-media-renderer/subtitles'
import {
  syncVideoElementAttributes,
  installPosterImageErrorHandler,
  syncBigPlaySuppressionAttribute,
} from '@/composables/video/video-js-media-renderer/playerAttributes'
import { syncEpisodeAutoplayControl } from '@/composables/video/video-js-media-renderer/autoplayMenu'
import {
  applySourceToPlayer,
} from '@/composables/video/video-js-media-renderer/sourceApplication'
import {
  destroyActivePlayer,
  markVideoInitialLoadComplete,
  markVideoInitialLoadStart,
} from '@/composables/video/video-js-media-renderer/playerLifecycle'

/** Interval between attempts to attach the dash.js quality bridge. */
const DASH_QUALITY_BRIDGE_RETRY_INTERVAL = 250
/** Bounded window after which pending dash.js bridge attach attempts stop. */
const DASH_QUALITY_BRIDGE_RETRY_TIMEOUT = 10_000

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
  /** Direction of the prev/next control currently previewed, or null when none is. */
  const previewedNavigationDirection = shallowRef<-1 | 1 | null>(null)

  /** Cleanup function for pending source restore operation. */
  let pendingSourceRestoreCleanup: (() => void) | null = null
  /** Cleanup function for pending quality selector override. */
  let pendingQualitySelectorCleanup: (() => void) | null = null
  /** dash.js quality bridge attached to the active DASH source. */
  let activeDashQualityBridge: DashQualityBridge | null = null
  /** Cleanup function for the pending DASH quality bridge installation. */
  let pendingDashQualityBridgeCleanup: (() => void) | null = null
  /** Sprite thumbnail plugin instance attached to the active player. */
  let spriteThumbnailsPlugin: VideoJsSpriteThumbnailsPlugin | null = null
  /** Remote subtitle tracks explicitly owned by the active renderer. */
  let remoteSubtitleTracks: VideoJsRemoteTextTrackHandle[] = []
  /** Preferred quality selected by the user. */
  let preferredQuality: string | null = null
  /** Latest state used to restore tracks after DASH renews the Video.js track lists. */
  let retainedPlayerState: VideoJsPlayerState | null = null
  /** Whether the video initial load complete event has been emitted. */
  const hasEmittedInitialLoadComplete = shallowRef(false)
  /** Whether the video metadata loaded event has been emitted. */
  const hasEmittedCurrentSourceMetadata = shallowRef(false)
  /** Monotonic identifier used to discard stale asynchronous storyboard loads. */
  let storyboardLoadId = 0
  /** Exact VTT cue geometry used to override the plugin's uniform interval calculation. */
  let storyboardVttCues: StoryboardVttCue[] = []
  /** Resolved VTT storyboard grid info (cell size + grid dimensions). */
  const vttStoryboardGrid = shallowRef<{
      width: number; height: number; columns: number; rows: number
  } | null>(null)
  /** Fixed 16:9 storyboard preview size used for the tooltip layout box. */
  const storyboardPreviewSize = computed(() => {
      // Read the player width so the preview follows the plugin responsive factor
      // applied below 600 px wide players.
      const playerWidth = activePlayer.value?.currentWidth() ?? 0
      const grid = vttStoryboardGrid.value
      const storyboard = props.source.storyboard
      const cellWidth = grid?.width ?? storyboard?.width
      const cellHeight = grid?.height ?? storyboard?.height
      const columns = grid?.columns ?? storyboard?.columns ?? 0

      if (!cellWidth || !cellHeight) {
        return null
      }
      // Fit the cell preview into the fixed 16:9 frame with a uniform scale so the
      // source aspect ratio is preserved (an independent x/y scale would stretch
      // off-ratio cells and deform the thumbnail).
      const fitScale = Math.min(
        STORYBOARD_PREVIEW_WIDTH / cellWidth,
        STORYBOARD_PREVIEW_HEIGHT / cellHeight,
      )
      const responsiveScale = playerWidth > 0 && playerWidth < STORYBOARD_RESPONSIVE_WIDTH
        ? playerWidth / STORYBOARD_RESPONSIVE_WIDTH
        : 1
      const scale = fitScale * responsiveScale

      return {
        displayWidth: scale * cellWidth,
        displayHeight: scale * cellHeight,
        columns,
        fitScale,
        responsiveScale,
        displayToSourceScale: scale,
      }
    })

   /**
    * Emits the video initial load complete event once per player instance.
    */
   function emitVideoInitialLoadComplete() {
     if (!hasEmittedInitialLoadComplete.value) {
       hasEmittedInitialLoadComplete.value = true
       emit('video-initial-load-complete')
     }
   }
   /**
    * Emits the latest persisted Video.js UI state to the parent controller.
    *
    * @param player Video.js player currently bound to the renderer.
    */
   function emitCurrentPlayerState(player: VideoJsPlayer) {
     const state = capturePlayerState(player, preferredQuality)
     emit('update:player-state', state)
   }
   /**
    * Emits the natural video dimensions once metadata is available.
    */
   function emitVideoMetadataLoaded() {
     if (hasEmittedCurrentSourceMetadata.value) {
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
     hasEmittedCurrentSourceMetadata.value = true
   }
   /**
    * Indicates whether the logo overlay should stay mounted before playback starts.
    */
   const shouldRenderPosterOverlay = computed(() =>
     Boolean(props.overlayLogoUrl),
   )

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
    * Releases the dash.js quality bridge attached to the active source.
    */
   function clearPendingDashQualityBridge() {
     pendingDashQualityBridgeCleanup?.()
     pendingDashQualityBridgeCleanup = null
     activeDashQualityBridge = null
   }
   /**
    * Applies the reactive accessibility and autoplay attributes directly to the video element.
    *
    * @param element Native video element managed by Video.js.
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
         previewedNavigationDirection.value = isPreviewed ? -1 : null
       },
       onNextVideoPreviewChange: (isPreviewed) => {
         previewedNavigationDirection.value = isPreviewed ? 1 : null
       },
     })
   }
   /**
    * Starts playback when autoplay is enabled and the current source is ready enough.
    *
    * @param player Video.js player currently bound to the renderer.
    * @param autoplayOverride Indicates that the preceding source was playing.
    */
   function attemptAutoplay(player: VideoJsPlayer, autoplayOverride?: boolean) {
     if (!props.autoplay && !autoplayOverride) {
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
    * @param quality Preferred quality captured from the previous video state.
    * @param expectedSourceUrl Source URL that should receive the selector override.
    */
   function configureQualityPreferenceSelector(
     player: VideoJsPlayer,
     quality: string | null,
     expectedSourceUrl: string,
   ) {
     clearPendingQualitySelectorCleanup()

     const qualityPreferenceToken = resolveQualityPreferenceToken(quality)

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
    * Attaches the dash.js quality bridge to the DASH source just assigned to the player.
    *
    * The source handler creates its dash.js MediaPlayer asynchronously after
    * `player.src()` is applied, so the installation is retried on a short
    * interval until the engine is ready or the bounded window expires. Each
    * source application clears pending attempts, so stale retries never attach
    * a bridge to a replaced source.
    *
    * @param player Video.js player currently bound to the renderer.
    * @param quality Preferred quality captured from the previous video state.
    */
   function installDashQualityBridgeForSource(player: VideoJsPlayer, quality: string | null) {
     clearPendingDashQualityBridge()

     let elapsed = 0

     const retryTimer = window.setInterval(() => {
       elapsed += DASH_QUALITY_BRIDGE_RETRY_INTERVAL

       if (elapsed > DASH_QUALITY_BRIDGE_RETRY_TIMEOUT) {
         clearPendingDashQualityBridge()
         return
       }
       if (activePlayer.value !== player) {
         clearPendingDashQualityBridge()
         return
       }
       const bridge = installDashQualityBridge({
         player,
         preferredQuality: quality,
         restoreTrackPreferences: () => {
           if (activePlayer.value === player) {
             restorePersistedTrackPreferences(player, retainedPlayerState)
           }
         },
       })

       if (!bridge) {
         return
       }
       window.clearInterval(retryTimer)
       activeDashQualityBridge = bridge
       pendingDashQualityBridgeCleanup = () => {
         bridge.dispose()
         activeDashQualityBridge = null
         pendingDashQualityBridgeCleanup = null
       }
     }, DASH_QUALITY_BRIDGE_RETRY_INTERVAL)

     pendingDashQualityBridgeCleanup = () => {
       window.clearInterval(retryTimer)
       activeDashQualityBridge = null
       pendingDashQualityBridgeCleanup = null
     }
   }
   /**
    * Restores one persisted player state after a source switch or component remount.
    *
    * @param player Video.js player currently bound to the renderer.
    * @param state Persisted player state that should be restored.
    */

   /**
    * Builds the Video.js source descriptor expected by the source handlers.
    *
    * DASH sources carry their Widevine license URL through `keySystemOptions`, which
    * the DASH source handler forwards to dash.js. HLS and progressive sources keep the
    * Video.js native/EME path.
    *
    * @param source Resolved source selected for playback.
    * @param spriteThumbnailOptions Resolved storyboard configuration for the sprite plugin.
    * @returns Video.js source configuration.
    */
   async function applySourceToPlayerWrapper(
     player: VideoJsPlayer,
     source: ResolvedVideoMediaSource,
     playerState: VideoJsPlayerState | null,
     switchTime?: number | null,
     switchAutoplay?: boolean,
   ): Promise<void> {
     const result = await applySourceToPlayer(
       player, source, playerState, switchTime, switchAutoplay,
       props, emit, spriteThumbnailsPlugin, activePlayer.value, activeDashQualityBridge,
       storyboardLoadId, preferredQuality, retainedPlayerState,
       vttStoryboardGrid, storyboardVttCues, remoteSubtitleTracks,
       clearPendingSourceRestoreCleanup, clearPendingQualitySelectorCleanup,
       clearPendingDashQualityBridge,
        () => markVideoInitialLoadStart(isVideoInitialLoading, hasEmittedCurrentSourceMetadata, hostElement, syncBigPlaySuppressionAttribute),
       () => markVideoInitialLoadComplete(isVideoInitialLoading, hostElement, props.isExternalLoading ?? false, syncBigPlaySuppressionAttribute), emitCurrentPlayerState,
       syncExistingPlayerConfiguration, installDashQualityBridgeForSource,
       configureQualityPreferenceSelector, emitVideoMetadataLoaded, attemptAutoplay,
      )
     storyboardLoadId = result.storyboardLoadId
     preferredQuality = result.preferredQuality
     retainedPlayerState = result.retainedPlayerState
     storyboardVttCues = result.storyboardVttCues
     remoteSubtitleTracks = result.remoteSubtitleTracks
     pendingSourceRestoreCleanup = result.pendingSourceRestoreCleanup
   }
  
    function createPlayer(element: HTMLVideoElement, source: ResolvedVideoMediaSource) {
      syncVideoElementAttributes(element, props)
      isPosterOverlayVisible.value = shouldRenderPosterOverlay.value && !props.autoplay
      markVideoInitialLoadStart(isVideoInitialLoading, hasEmittedCurrentSourceMetadata, hostElement, syncBigPlaySuppressionAttribute)

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
        markVideoInitialLoadComplete(isVideoInitialLoading, hostElement, props.isExternalLoading ?? false, syncBigPlaySuppressionAttribute)
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
             spriteThumbnailsPlugin = player.spriteThumbnails({}) ?? null
          }
          installStoryboardVttCueOverride(player, storyboardPreviewSize.value, storyboardVttCues)

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
           preferredQuality = getSelectedQualityLabel(player)
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

       void applySourceToPlayerWrapper(player, source, props.initialPlayerState ?? null)
     })
   }
   watch(
     () => videoElement.value,
     (element) => {
       const _dlr = destroyActivePlayer(activePlayer, remoteSubtitleTracks, clearPendingSourceRestoreCleanup, clearPendingQualitySelectorCleanup, clearPendingDashQualityBridge, spriteThumbnailsPlugin, retainedPlayerState, isPosterOverlayVisible, hasEmittedInitialLoadComplete.value, previewedNavigationDirection); remoteSubtitleTracks = _dlr.remoteSubtitleTracks; spriteThumbnailsPlugin = _dlr.spriteThumbnailsPlugin; retainedPlayerState = _dlr.retainedPlayerState

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
       const playerState = capturePlayerState(player, preferredQuality)
       emitPlayerState(playerState)

       const currentTime = player.currentTime()
       const pendingTime = props.preservePlaybackOnSourceSwitch &&
         typeof currentTime === 'number' && Number.isFinite(currentTime) && currentTime > 0
         ? currentTime
         : null
       const pendingAutoplay = props.preservePlaybackOnSourceSwitch &&
         typeof player.paused === 'function' && !player.paused()

       void applySourceToPlayerWrapper(player, nextSource, playerState, pendingTime, pendingAutoplay)
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
       syncBigPlaySuppressionAttribute(hostElement, isVideoInitialLoading.value || (isExternalLoading ?? false))
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
           previewedNavigationDirection.value = isPreviewed ? -1 : null
         },
         onNextVideoPreviewChange: (isPreviewed) => {
           previewedNavigationDirection.value = isPreviewed ? 1 : null
         },
       })
     },
   )

    onBeforeUnmount(() => {
      const _dlr = destroyActivePlayer(activePlayer, remoteSubtitleTracks, clearPendingSourceRestoreCleanup, clearPendingQualitySelectorCleanup, clearPendingDashQualityBridge, spriteThumbnailsPlugin, retainedPlayerState, isPosterOverlayVisible, hasEmittedInitialLoadComplete.value, previewedNavigationDirection); remoteSubtitleTracks = _dlr.remoteSubtitleTracks; spriteThumbnailsPlugin = _dlr.spriteThumbnailsPlugin; retainedPlayerState = _dlr.retainedPlayerState
    })

   return {
     isPosterOverlayVisible,
     shouldRenderPosterOverlay,
     isVideoInitialLoading,
     storyboardPreviewSize,
     vttStoryboardGrid,
     previewedNavigationDirection,
   }
}
