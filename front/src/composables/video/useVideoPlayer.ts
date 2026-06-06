import { computed, ref, useAttrs, watch, type ComputedRef } from 'vue'

import type { EntryPlayer } from '@/types/entry'
import type { EntryPlayerLanguageOption } from '@/composables/entry-details/entryVideoPlayer'
import type {
  ResolvedIframeMediaSource,
  ResolvedPlayerMediaSource,
  ResolvedVideoMediaSource,
} from '@/services/players'
import { useStorage, type VideoPlayerPreferences } from '@/services/storage'
import type { VideoJsPlayerState } from '@/types/media'
import { t } from '@/i18n'

type MediaIframeReferrerPolicy =
  | 'no-referrer'
  | 'no-referrer-when-downgrade'
  | 'origin'
  | 'origin-when-cross-origin'
  | 'same-origin'
  | 'strict-origin'
  | 'strict-origin-when-cross-origin'
  | 'unsafe-url'

const entryDetailsIframeAllow =
  'accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share'

/**
 * Builds the initial Video.js state restored from durable player preferences.
 *
 * @param preferences Player preferences persisted in local storage.
 * @returns Player state shape expected by the Video.js renderer.
 */
function createInitialVideoPlayerState(
  preferences: VideoPlayerPreferences,
): VideoJsPlayerState {
  return {
    volume: preferences.volume,
    muted: preferences.muted,
    playbackRate: 1,
    qualityLabel: preferences.qualityLabel,
    audioTrack: preferences.audioTrack,
    textTrack: preferences.textTrack,
    textTrackSettings: preferences.textTrackSettings,
    isFullscreen: false,
    showsRemainingTime: false,
  }
}

/**
 * Extracts only the player state fields that should be persisted between sessions.
 *
 * @param playerState Latest Video.js renderer state.
 * @returns Durable player preferences safe to store locally.
 */
function createVideoPlayerPreferences(playerState: VideoJsPlayerState): VideoPlayerPreferences {
  return {
    volume: playerState.volume,
    muted: playerState.muted,
    qualityLabel: playerState.qualityLabel,
    audioTrack: playerState.audioTrack,
    textTrack: playerState.textTrack,
    textTrackSettings: playerState.textTrackSettings,
  }
}

/**
 * Props accepted by the video player composable.
 */
interface VideoPlayerProps {
  /**
   * Resolved media source describing how one standalone media surface should be rendered.
   * @default null
   */
  source?: ResolvedPlayerMediaSource | null
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
  referrerPolicy?: MediaIframeReferrerPolicy | null
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
  mediaSource?: ResolvedPlayerMediaSource | null
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
  availableLanguages: EntryPlayerLanguageOption[]
  /**
   * Active language key selected in the entry details player.
   * @default null
   */
  activeLanguageKey?: string | null
  /**
   * Player options rendered by the entry details player selector.
   * @default []
   */
  filteredPlayers: EntryPlayer[]
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
}

/**
 * Emits accepted by the video player composable.
 */
interface VideoPlayerEmits {
  (evt: 'update:active-language-key', value: string | null): void
  (evt: 'remember-current-language'): void
  (evt: 'update:active-player-id', value: string | null): void
  (evt: 'remember-current-player'): void
  (evt: 'update:playback-progress', value: number | null): void
  (evt: 'update:is-episode-autoplay-enabled', value: boolean): void
  (evt: 'playback-started', sourceUrl: string | null): void
  (evt: 'playback-ended'): void
}

/**
 * Composable for managing video player state and logic.
 */
export function useVideoPlayer(props: VideoPlayerProps, emit: VideoPlayerEmits) {
  const attrs = useAttrs()
  const storage = useStorage()
  const lastResolvedMediaSource = ref<ResolvedPlayerMediaSource | null>(null)

  // Ensure the ref values are properly typed
  type LastResolvedMediaSource = ResolvedPlayerMediaSource | null

  watch(
    () => props.mediaSource,
    (nextMediaSource) => {
      if (nextMediaSource) {
        lastResolvedMediaSource.value = nextMediaSource
      }
    },
    { immediate: true },
  )

  /**
   * Indicates whether the component should render the entry details player layout.
   */
  const isEntryDetailsMode = computed(() =>
    props.displayTitle !== null ||
    props.showTrailerPlayer ||
    props.showMediaPlayer ||
    props.mediaOpenUrl !== null ||
    props.showPlayerControls ||
    props.availableLanguages.length > 0 ||
    props.filteredPlayers.length > 0,
  )

  /**
   * Limits stored audio and quality preferences to player surfaces controlled by the user.
   */
  const shouldUseStoredPlayerPreferences = computed(() =>
    isEntryDetailsMode.value || Boolean(props.controls),
  )

  const persistedVideoPlayerState = ref<VideoJsPlayerState | null>(
    shouldUseStoredPlayerPreferences.value
      ? createInitialVideoPlayerState(storage.getVideoPlayerPreferences())
      : null,
  )

  /**
   * Exposes a stable title used by accessibility labels in the entry details player mode.
   */
  const detailTitle = computed(() => props.displayTitle ?? t('player.defaultDisplayTitle'))

  /**
   * Indicates whether the entry details selector bar should be rendered.
   */
  const showDetailsPlayerControls = computed(() =>
    props.showPlayerControls || Boolean(props.mediaOpenUrl),
  )

  /**
   * Keeps the media surface mounted through one explicit episode transition while the next media
   * source is being resolved.
   */
  const shouldKeepMediaSurfaceMountedDuringTransition = computed(() =>
    props.preferPersistedMediaSurface &&
    !props.mediaPlayerErrorMessage &&
    Boolean(lastResolvedMediaSource.value) &&
    (
      props.isMediaPlayerLoading ||
      !props.mediaSource
    ),
  )

/**
 * Keeps the latest playable media source mounted while the next source or page state settles.
 */
const renderedMediaSource = (computed as any)(() => {
  if (props.mediaSource !== null && props.mediaSource !== undefined) {
    return props.mediaSource
  }
  if (shouldKeepMediaSurfaceMountedDuringTransition.value) {
    const value = lastResolvedMediaSource.value
    if (value !== null && value !== undefined) {
      return value
    }
  }
  return null
})

  type EntryDetailsSurfaceMode = 'media' | 'trailer'
  type ActiveSurfaceMode = EntryDetailsSurfaceMode | 'standalone'

  /**
   * Prevents transient page updates from unmounting the media surface while it is still active.
   */
  const shouldRenderMediaSurface = computed(() =>
    props.showMediaPlayer || shouldKeepMediaSurfaceMountedDuringTransition.value,
  )

  /**
   * Prevents the trailer fallback from replacing the media surface during one episode transition.
   */
  const shouldRenderTrailerSurface = computed(() =>
    props.showTrailerPlayer && !shouldKeepMediaSurfaceMountedDuringTransition.value,
  )

  /**
   * Indicates which entry-details surface currently owns the player area.
   */
  const entryDetailsSurfaceMode = computed<EntryDetailsSurfaceMode | null>(() => {
    if (shouldRenderMediaSurface.value) {
      return 'media'
    }

    if (shouldRenderTrailerSurface.value) {
      return 'trailer'
    }

    return null
  })

  /**
   * Exposes the current render mode for the unified surface template.
   */
  const activeSurfaceMode = computed<ActiveSurfaceMode | null>(() => {
    if (isEntryDetailsMode.value) {
      return entryDetailsSurfaceMode.value
    }

    return props.source ? 'standalone' : null
  })

  /**
   * Exposes the one source currently rendered by the unified surface template.
   */
  const activeSurfaceSource = computed<ResolvedPlayerMediaSource | null>(() => {
    if (!isEntryDetailsMode.value) {
      return props.source
    }

    if (entryDetailsSurfaceMode.value === 'media') {
      return renderedMediaSource.value
    }

    if (entryDetailsSurfaceMode.value === 'trailer') {
      return props.mediaSource
    }

    return null
  })

  /**
   * Indicates whether the shared surface wrapper should be rendered.
   */
  const shouldRenderSurface = computed(() =>
    isEntryDetailsMode.value
      ? entryDetailsSurfaceMode.value !== null
      : Boolean(props.source),
  )

  /**
   * Indicates whether the current entry-details surface should display the loading overlay.
   */
  const shouldRenderSurfaceLoadingState = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.isMediaPlayerLoading,
  )

  /**
   * Indicates whether the current entry-details surface should display the error state.
   */
  const shouldRenderSurfaceErrorState = computed(() =>
    entryDetailsSurfaceMode.value === 'media' &&
    Boolean(props.mediaPlayerErrorMessage) &&
    !activeSurfaceSource.value,
  )

  /**
   * Exposes the classes applied to the shared surface wrapper.
   */
  const surfaceContainerClass = computed(() =>
    isEntryDetailsMode.value
      ? 'entry-details__trailer'
      : 'video-player__surface--standalone',
  )

  /**
   * Exposes the classes applied to the loading state overlay when media stays mounted underneath.
   */
  const surfaceStateClasses = computed(() => ({
    'entry-details__player-state--overlay':
      shouldRenderSurfaceLoadingState.value && Boolean(activeSurfaceSource.value),
  }))

  /**
   * Ensures the renderer remounts only when the displayed surface context actually changes.
   */
  const activeSurfaceRendererKey = computed(() =>
    activeSurfaceMode.value && activeSurfaceSource.value
      ? `${activeSurfaceMode.value}:${activeSurfaceSource.value.renderer}`
      : 'surface:none',
  )

  /**
   * Exposes the iframe variant of the current surface source when selected.
   */
  const activeIframeSource = computed<ResolvedIframeMediaSource | null>(() =>
    activeSurfaceSource.value?.renderer === 'iframe'
      ? activeSurfaceSource.value
      : null,
  )

  /**
   * Exposes the Video.js variant of the current surface source when selected.
   */
  const activeVideoSource = computed<ResolvedVideoMediaSource | null>(() =>
    activeSurfaceSource.value?.renderer === 'video'
      ? activeSurfaceSource.value
      : null,
  )

  /**
   * Exposes the optional attrs forwarded only by standalone renderers.
   */
  const standaloneRendererAttrs = computed(() =>
    isEntryDetailsMode.value ? undefined : attrs,
  )

  /**
   * Exposes the title applied to the unified iframe renderer.
   */
  const activeIframeTitle = computed(() => {
    if (!isEntryDetailsMode.value) {
      return props.iframeTitle
    }

    if (entryDetailsSurfaceMode.value === 'trailer') {
      return t('player.trailerTitle', { title: detailTitle.value }) 
    }

    if (entryDetailsSurfaceMode.value === 'media') {
      return t('media.videoTitle', { title: detailTitle.value })
    }

    return null
  })

  /**
   * Exposes the permissions string applied to the unified iframe renderer.
   */
  const activeIframeAllow = computed(() =>
    isEntryDetailsMode.value ? entryDetailsIframeAllow : props.allow,
  )

  /**
   * Indicates whether the unified iframe renderer should allow fullscreen.
   */
  const activeIframeAllowFullscreen = computed(() =>
    isEntryDetailsMode.value || props.allowFullscreen,
  )

  /**
   * Exposes the loading strategy applied to the unified iframe renderer.
   */
  const activeIframeLoading = computed(() =>
    isEntryDetailsMode.value ? 'lazy' : props.loading,
  )

/**
 * Exposes the referrer policy applied to the unified iframe renderer.
 */
const activeIframeReferrerPolicy = computed<MediaIframeReferrerPolicy | null>(() => {
  const result = isEntryDetailsMode.value ? 'strict-origin-when-cross-origin' : props.referrerPolicy
  return result ?? null
})

  /**
   * Exposes the accessibility attributes applied to the unified iframe renderer.
   */
  const activeIframeAriaHidden = computed(() =>
    isEntryDetailsMode.value ? false : props.ariaHidden,
  )

  const activeIframeTabIndex = computed(() =>
    isEntryDetailsMode.value ? null : props.tabIndex,
  )

  /**
   * Exposes the classes applied to the unified iframe renderer.
   */
  const activeIframeClass = computed(() =>
    isEntryDetailsMode.value ? 'entry-details__trailer-frame' : 'video-player__iframe--standalone',
  )

  /**
   * Exposes the accessibility label applied to the unified Video.js renderer.
   */
  const activeVideoAriaLabel = computed(() => {
    if (!isEntryDetailsMode.value) {
      return props.videoAriaLabel
    }
    
    if (entryDetailsSurfaceMode.value === 'trailer') {
      return t('player.trailerTitle', { title: detailTitle.value }) 
    }

    if (entryDetailsSurfaceMode.value === 'media') {
      return t('media.videoTitle', { title: detailTitle.value })
    }

    return null
  })

  /**
   * Exposes the props forwarded to the unified Video.js renderer.
   */
  const activeVideoPoster = computed(() =>
    entryDetailsSurfaceMode.value === 'media' ? props.mediaPosterUrl : props.poster,
  )

  const activeVideoOverlayLogoUrl = computed(() =>
    (entryDetailsSurfaceMode.value === 'media' || entryDetailsSurfaceMode.value === 'trailer') ? props.mediaOverlayLogoUrl : null,
  )

  const activeVideoAutoplay = computed(() =>
    entryDetailsSurfaceMode.value === 'media' ? props.mediaAutoplay :
    entryDetailsSurfaceMode.value === 'trailer' ? true : props.autoplay,
  )

  const activeVideoMuted = computed(() =>
    isEntryDetailsMode.value ? false : props.muted,
  )

  const activeVideoLoop = computed(() =>
    isEntryDetailsMode.value ? false : props.loop,
  )

  const activeVideoControls = computed(() =>
    isEntryDetailsMode.value ? true : props.controls,
  )

  const activeVideoPlaysinline = computed(() =>
    isEntryDetailsMode.value ? true : props.playsinline,
  )

  const activeVideoPreload = computed(() =>
    isEntryDetailsMode.value ? 'metadata' : props.preload,
  )

  const activeVideoAriaHidden = computed(() =>
    isEntryDetailsMode.value ? false : props.ariaHidden,
  )

  const activeVideoTabIndex = computed(() =>
    isEntryDetailsMode.value ? null : props.tabIndex,
  )

  const activeVideoInitialPlaybackTime = computed(() =>
    !isEntryDetailsMode.value || entryDetailsSurfaceMode.value === 'media'
      ? props.initialPlaybackTime
      : null,
  )

  const activeVideoShowEpisodeAutoplayToggle = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.showEpisodeAutoplayToggle,
  )

   const activeVideoIsEpisodeAutoplayEnabled = computed(() =>
     entryDetailsSurfaceMode.value === 'media' && props.isEpisodeAutoplayEnabled,
   )

   /**
    * Indicates whether the big play button should be displayed on the Video.js player.
    * Controls visibility based on the controls prop, with CSS handling loading state.
    */
   const activeVideoShowBigPlayButton = computed(() => activeVideoControls.value)

   /**
    * Exposes the classes applied to the unified Video.js renderer.
    */
   const activeVideoClass = computed(() =>
     isEntryDetailsMode.value
       ? 'entry-details__trailer-frame entry-details__trailer-frame--video'
       : 'video-player__video--standalone',
   )

  /**
   * Indicates whether the entry-details picker should stay visible below the active media surface.
   */
  const shouldShowDetailsPlayerPicker = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && showDetailsPlayerControls.value,
  )

  /**
   * Indicates whether the active Video.js renderer should forward progress and lifecycle events.
   */
  const shouldForwardPrimaryVideoEvents = computed(() =>
    !isEntryDetailsMode.value || entryDetailsSurfaceMode.value === 'media',
  )

/**
 * Exposes a stable language key to the native select element.
 */
const languageModel = computed(() =>
  props.activeLanguageKey ?? props.availableLanguages?.[0]?.key ?? '',
)

/**
 * Exposes a stable player identifier to the native select element.
 */
const playerModel = computed(() => props.activePlayerId ?? props.filteredPlayers?.[0]?.id ?? '')

/**
 * Stores the latest Video.js UI state so it can be restored after a source switch.
 *
 * @param value Latest captured player state, or `null` when unavailable.
 */
const handlePlayerStateUpdate = (value: VideoJsPlayerState | null) => {
  persistedVideoPlayerState.value = value

  if (value && shouldUseStoredPlayerPreferences.value) {
    storage.setVideoPlayerPreferences(createVideoPlayerPreferences(value))
  }
}

/**
 * Stores the newly selected language in the parent controller.
 *
 * @param event Native select change event.
 */
const handleLanguageChange = (event: Event) => {
  const target = event.target as HTMLSelectElement

  emit('update:active-language-key', target.value || null)
  emit('remember-current-language')
}

/**
 * Stores the newly selected player in the parent controller.
 *
 * @param event Native select change event.
 */
const handlePlayerChange = (event: Event) => {
  const target = event.target as HTMLSelectElement

  emit('update:active-player-id', target.value || null)
  emit('remember-current-player')
}

/**
 * Forwards the current Video.js playback position to the entry details controller.
 *
 * @param value Playback position in seconds, or `null` when it should be cleared.
 */
const handlePlaybackProgressUpdate = (value: number | null) => {
  emit('update:playback-progress', value)
}

/**
 * Forwards the effective playback start notification to the parent controller.
 */
const handlePlaybackStarted = (sourceUrl: string | null) => {
  emit('playback-started', sourceUrl)
}

/**
 * Forwards the playback-ended notification to the parent controller.
 */
const handlePlaybackEnded = () => {
  emit('playback-ended')
}

/**
 * Forwards the episode autoplay preference update emitted by the embedded Video.js control bar.
 *
 * @param value Indicates whether automatic playback of the next episode is enabled.
 */
const handleEpisodeAutoplayEnabledUpdate = (value: boolean) => {
  emit('update:is-episode-autoplay-enabled', value)
}

/**
 * Forwards the active Video.js playback progress only when the current surface is a primary video.
 *
 * @param value Playback position in seconds, or `null` when it should be cleared.
 */
const handleActiveVideoPlaybackProgressUpdate = (value: number | null) => {
  if (!shouldForwardPrimaryVideoEvents.value) {
    return
  }

  handlePlaybackProgressUpdate(value)
}

/**
 * Forwards the active Video.js playback-started notification only when it belongs to the primary video.
 */
const handleActiveVideoPlaybackStarted = (sourceUrl: string | null) => {
  if (!shouldForwardPrimaryVideoEvents.value) {
    return
  }

  handlePlaybackStarted(sourceUrl)
}

/**
 * Forwards the active Video.js playback-ended notification only when it belongs to the primary video.
 */
const handleActiveVideoPlaybackEnded = () => {
  if (!shouldForwardPrimaryVideoEvents.value) {
    return
  }

  handlePlaybackEnded()
}

/**
 * Forwards the episode autoplay preference update only when the primary media surface is active.
 *
 * @param value Indicates whether automatic playback of the next episode is enabled.
 */
const handleActiveVideoEpisodeAutoplayEnabledUpdate = (value: boolean) => {
  if (entryDetailsSurfaceMode.value !== 'media') {
    return
  }

  handleEpisodeAutoplayEnabledUpdate(value)
}

   return {
     // State
     persistedVideoPlayerState,

     // Computed properties
     isEntryDetailsMode,
     detailTitle,
     showDetailsPlayerControls,
     shouldKeepMediaSurfaceMountedDuringTransition,
     renderedMediaSource,
     shouldRenderMediaSurface,
     shouldRenderTrailerSurface,
     entryDetailsSurfaceMode,
     activeSurfaceMode,
     activeSurfaceSource,
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
     activeVideoShowEpisodeAutoplayToggle,
     activeVideoIsEpisodeAutoplayEnabled,
     activeVideoShowBigPlayButton,
     activeVideoClass,
     shouldShowDetailsPlayerPicker,
     shouldForwardPrimaryVideoEvents,
     languageModel,
     playerModel,

     // Functions
     handlePlayerStateUpdate,
     handleLanguageChange,
     handlePlayerChange,
     handlePlaybackProgressUpdate,
     handlePlaybackStarted,
     handlePlaybackEnded,
     handleEpisodeAutoplayEnabledUpdate,
     handleActiveVideoPlaybackProgressUpdate,
     handleActiveVideoPlaybackStarted,
     handleActiveVideoPlaybackEnded,
     handleActiveVideoEpisodeAutoplayEnabledUpdate,
   }
}
