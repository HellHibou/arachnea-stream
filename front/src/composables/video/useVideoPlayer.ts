import { computed, ref, shallowRef, useAttrs, watch, type ComputedRef } from 'vue'

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

/** Referrer policy options for iframe elements. */
type MediaIframeReferrerPolicy =
  | 'no-referrer'
  | 'no-referrer-when-downgrade'
  | 'origin'
  | 'origin-when-cross-origin'
  | 'same-origin'
  | 'strict-origin'
  | 'strict-origin-when-cross-origin'
  | 'unsafe-url'

/** Default allow attribute for iframes in entry details mode. */
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
    quality: preferences.quality,
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
    quality: playerState.quality,
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
 }

/**
 * Emits accepted by the video player composable.
 */
interface VideoPlayerEmits {
  /** Emitted when the active language key changes. */
  (evt: 'update:active-language-key', value: string | null): void
  /** Emitted when the current language should be remembered for future use. */
  (evt: 'remember-current-language'): void
  /** Emitted when the active player identifier changes. */
  (evt: 'update:active-player-id', value: string | null): void
  /** Emitted when the current player should be remembered for future use. */
  (evt: 'remember-current-player'): void
  /** Emitted when playback progress updates. */
  (evt: 'update:playback-progress', value: number | null): void
  /** Emitted when the episode autoplay enabled state changes. */
  (evt: 'update:is-episode-autoplay-enabled', value: boolean): void
  /** Emitted when playback starts. */
  (evt: 'playback-started', sourceUrl: string | null): void
  /** Emitted when playback ends. */
  (evt: 'playback-ended'): void
  /** Emitted when video navigation is requested via control bar buttons. */
  (evt: 'navigate-video', direction: -1 | 1): void
}

/**
 * Composable for managing video player state and logic.
 *
 * @param props - Video player configuration props.
 * @param emit - Event emitter for player events.
 * @returns Object containing state, computed properties, and handler functions.
 */
export function useVideoPlayer(props: VideoPlayerProps, emit: VideoPlayerEmits) {
  const attrs = useAttrs()
  const storage = useStorage()
  /** The last resolved media source, used to keep the media surface mounted during transitions. */
  const lastResolvedMediaSource = ref<ResolvedPlayerMediaSource | null>(null)
  /** Whether a language or player selection is waiting for its next source to resolve. */
  const isPlayerSelectionChangePending = shallowRef(false)
  /** Source URL whose preceding playback state should survive a player selection change. */
  const sourcePreservingPlayback = shallowRef<string | null>(null)

  /** Type for the last resolved media source reference. */
  type LastResolvedMediaSource = ResolvedPlayerMediaSource | null

  watch(
    () => props.mediaSource,
    (nextMediaSource) => {
      if (nextMediaSource) {
        lastResolvedMediaSource.value = nextMediaSource

        if (isPlayerSelectionChangePending.value) {
          sourcePreservingPlayback.value = nextMediaSource.src
        }

        isPlayerSelectionChangePending.value = false
      }
    },
    { immediate: true },
  )

  /**
   * Indicates whether the component should render the entry details player layout.
   *
   * @returns True when any entry details player feature is active.
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
   *
   * @returns True when player preferences should be loaded and saved.
   */
  const shouldUseStoredPlayerPreferences = computed(() =>
    isEntryDetailsMode.value || Boolean(props.controls),
  )

  /** Persisted video player state from storage. */
  const persistedVideoPlayerState = ref<VideoJsPlayerState | null>(
    shouldUseStoredPlayerPreferences.value
      ? createInitialVideoPlayerState(storage.getVideoPlayerPreferences())
      : null,
  )

  /**
   * Exposes a stable title used by accessibility labels in the entry details player mode.
   *
   * @returns The display title or a default translation.
   */
  const detailTitle = computed(() => props.displayTitle ?? t('player.defaultDisplayTitle'))

  /**
   * Indicates whether the entry details selector bar should be rendered.
   *
   * @returns True when player controls or media open URL is available.
   */
  const showDetailsPlayerControls = computed(() =>
    props.showPlayerControls || Boolean(props.mediaOpenUrl),
  )

  /**
   * Keeps the media surface mounted while an episode, language, or player transition resolves its
   * next media source.
   *
   * @returns True when a persisted media surface should be kept during transition.
   */
  const shouldKeepMediaSurfaceMountedDuringTransition = computed(() =>
    (props.preferPersistedMediaSurface || isPlayerSelectionChangePending.value) &&
    !props.mediaPlayerErrorMessage &&
    Boolean(lastResolvedMediaSource.value) &&
    (
      props.isMediaPlayerLoading ||
      !props.mediaSource
    ),
  )

/**
 * Keeps the latest playable media source mounted while the next source or page state settles.
 *
 * @returns The active media source or null.
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

  /** Entry details surface mode type: either 'media' or 'trailer'. */
  type EntryDetailsSurfaceMode = 'media' | 'trailer'
  
  /** Active surface mode type including standalone: 'media', 'trailer', or 'standalone'. */
  type ActiveSurfaceMode = EntryDetailsSurfaceMode | 'standalone'

  /**
   * Prevents transient page updates from unmounting the media surface while it is still active.
   *
   * @returns True when the media surface should be rendered.
   */
  const shouldRenderMediaSurface = computed(() =>
    props.showMediaPlayer || shouldKeepMediaSurfaceMountedDuringTransition.value,
  )

  /**
   * Prevents the trailer fallback from replacing the media surface during one episode transition.
   *
   * @returns True when the trailer surface should be rendered.
   */
  const shouldRenderTrailerSurface = computed(() =>
    props.showTrailerPlayer && !shouldKeepMediaSurfaceMountedDuringTransition.value,
  )

  /**
   * Indicates which entry-details surface currently owns the player area.
   *
   * @returns The current surface mode: 'media', 'trailer', or null.
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
   *
   * @returns The active surface mode: 'media', 'trailer', 'standalone', or null.
   */
  const activeSurfaceMode = computed<ActiveSurfaceMode | null>(() => {
    if (isEntryDetailsMode.value) {
      return entryDetailsSurfaceMode.value
    }

    return props.source ? 'standalone' : null
  })

  /**
   * Exposes the one source currently rendered by the unified surface template.
   *
   * @returns The resolved player media source for the active surface.
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
   *
   * @returns True when a surface should be rendered.
   */
  const shouldRenderSurface = computed(() =>
    isEntryDetailsMode.value
      ? entryDetailsSurfaceMode.value !== null
      : Boolean(props.source),
  )

  /**
   * Indicates whether the current entry-details surface should display the loading overlay.
   *
   * @returns True when loading state should be shown.
   */
  const shouldRenderSurfaceLoadingState = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.isMediaPlayerLoading,
  )

  /**
   * Indicates whether the current entry-details surface should display the error state.
   *
   * @returns True when error state should be shown.
   */
  const shouldRenderSurfaceErrorState = computed(() =>
    entryDetailsSurfaceMode.value === 'media' &&
    Boolean(props.mediaPlayerErrorMessage) &&
    !activeSurfaceSource.value,
  )

  /**
   * Exposes the classes applied to the shared surface wrapper.
   *
   * @returns CSS class for the surface container.
   */
  const surfaceContainerClass = computed(() =>
    isEntryDetailsMode.value
      ? 'entry-details__trailer'
      : 'video-player__surface--standalone',
  )

  /**
   * Exposes the classes applied to the loading state overlay when media stays mounted underneath.
   *
   * @returns Object with CSS class modifiers.
   */
  const surfaceStateClasses = computed(() => ({
    'entry-details__player-state--overlay':
      shouldRenderSurfaceLoadingState.value && Boolean(activeSurfaceSource.value),
  }))

  /**
   * Ensures the renderer remounts only when the displayed surface context actually changes.
   *
   * @returns A key string identifying the current surface renderer.
   */
  const activeSurfaceRendererKey = computed(() =>
    activeSurfaceMode.value && activeSurfaceSource.value
      ? `${activeSurfaceMode.value}:${activeSurfaceSource.value.renderer}`
      : 'surface:none',
  )

  /**
   * Exposes the iframe variant of the current surface source when selected.
   *
   * @returns The iframe media source or null.
   */
  const activeIframeSource = computed<ResolvedIframeMediaSource | null>(() =>
    activeSurfaceSource.value?.renderer === 'iframe'
      ? activeSurfaceSource.value
      : null,
  )

  /**
   * Exposes the Video.js variant of the current surface source when selected.
   *
   * @returns The video media source or null.
   */
  const activeVideoSource = computed<ResolvedVideoMediaSource | null>(() =>
    activeSurfaceSource.value?.renderer === 'video'
      ? activeSurfaceSource.value
      : null,
  )

  /**
   * Exposes the optional attrs forwarded only by standalone renderers.
   *
   * @returns The attrs object or undefined for entry details mode.
   */
  const standaloneRendererAttrs = computed(() =>
    isEntryDetailsMode.value ? undefined : attrs,
  )

  /**
   * Exposes the title applied to the unified iframe renderer.
   *
   * @returns The appropriate title based on mode and surface.
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
   *
   * @returns The allow attribute value.
   */
  const activeIframeAllow = computed(() =>
    isEntryDetailsMode.value ? entryDetailsIframeAllow : props.allow,
  )

  /**
   * Indicates whether the unified iframe renderer should allow fullscreen.
   *
   * @returns True when fullscreen should be allowed.
   */
  const activeIframeAllowFullscreen = computed(() =>
    isEntryDetailsMode.value || props.allowFullscreen,
  )

  /**
   * Exposes the loading strategy applied to the unified iframe renderer.
   *
   * @returns The loading strategy: 'lazy' or 'eager'.
   */
  const activeIframeLoading = computed(() =>
    isEntryDetailsMode.value ? 'lazy' : props.loading,
  )

/**
 * Exposes the referrer policy applied to the unified iframe renderer.
 *
 * @returns The referrer policy or null.
 */
const activeIframeReferrerPolicy = computed<MediaIframeReferrerPolicy | null>(() => {
  const result = isEntryDetailsMode.value ? 'strict-origin-when-cross-origin' : props.referrerPolicy
  return result ?? null
})

  /**
   * Exposes the accessibility attributes applied to the unified iframe renderer.
   *
   * @returns Whether the iframe should be hidden from accessibility APIs.
   */
  const activeIframeAriaHidden = computed(() =>
    isEntryDetailsMode.value ? false : props.ariaHidden,
  )

  /**
   * Exposes the tab index applied to the unified iframe renderer.
   *
   * @returns The tab index or null for entry details mode.
   */
  const activeIframeTabIndex = computed(() =>
    isEntryDetailsMode.value ? null : props.tabIndex,
  )

  /**
   * Exposes the classes applied to the unified iframe renderer.
   *
   * @returns CSS class for the iframe.
   */
  const activeIframeClass = computed(() =>
    isEntryDetailsMode.value ? 'entry-details__trailer-frame' : 'video-player__iframe--standalone',
  )

  /**
   * Exposes the accessibility label applied to the unified Video.js renderer.
   *
   * @returns The appropriate aria label based on mode and surface.
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
   * Exposes the poster URL applied to the unified Video.js renderer.
   *
   * @returns The media poster URL in entry details mode, or the standalone poster URL.
   */
  const activeVideoPoster = computed(() =>
    entryDetailsSurfaceMode.value === 'media' ? props.mediaPosterUrl : props.poster,
  )

  /**
   * Exposes the overlay logo URL applied to the unified Video.js renderer.
   *
   * @returns The media overlay logo URL for entry details surfaces, or null for standalone.
   */
  const activeVideoOverlayLogoUrl = computed(() =>
    (entryDetailsSurfaceMode.value === 'media' || entryDetailsSurfaceMode.value === 'trailer') ? props.mediaOverlayLogoUrl : null,
  )

  /**
   * Exposes the autoplay setting applied to the unified Video.js renderer.
   *
   * @returns True for trailer in entry details mode, media autoplay setting for media surface, or standalone autoplay prop.
   */
  const activeVideoAutoplay = computed(() =>
    entryDetailsSurfaceMode.value === 'media' ? props.mediaAutoplay :
    entryDetailsSurfaceMode.value === 'trailer' ? true : props.autoplay,
  )

  /**
   * Exposes the muted setting applied to the unified Video.js renderer.
   *
   * @returns False for entry details mode, or the standalone muted prop.
   */
  const activeVideoMuted = computed(() =>
    isEntryDetailsMode.value ? false : props.muted,
  )

  /**
   * Exposes the loop setting applied to the unified Video.js renderer.
   *
   * @returns False for entry details mode, or the standalone loop prop.
   */
  const activeVideoLoop = computed(() =>
    isEntryDetailsMode.value ? false : props.loop,
  )

  /**
   * Exposes the controls visibility setting applied to the unified Video.js renderer.
   *
   * @returns True for entry details mode, or the standalone controls prop.
   */
  const activeVideoControls = computed(() =>
    isEntryDetailsMode.value ? true : props.controls,
  )

  /**
   * Exposes the playsinline setting applied to the unified Video.js renderer.
   *
   * @returns True for entry details mode, or the standalone playsinline prop.
   */
  const activeVideoPlaysinline = computed(() =>
    isEntryDetailsMode.value ? true : props.playsinline,
  )

  /**
   * Exposes the preload setting applied to the unified Video.js renderer.
   *
   * @returns 'metadata' for entry details mode, or the standalone preload prop.
   */
  const activeVideoPreload = computed(() =>
    isEntryDetailsMode.value ? 'metadata' : props.preload,
  )

  /**
   * Exposes the aria hidden setting applied to the unified Video.js renderer.
   *
   * @returns False for entry details mode, or the standalone ariaHidden prop.
   */
  const activeVideoAriaHidden = computed(() =>
    isEntryDetailsMode.value ? false : props.ariaHidden,
  )

  /**
   * Exposes the tab index applied to the unified Video.js renderer.
   *
   * @returns Null for entry details mode, or the standalone tabIndex prop.
   */
  const activeVideoTabIndex = computed(() =>
    isEntryDetailsMode.value ? null : props.tabIndex,
  )

  /**
   * Exposes the initial playback time applied to the unified Video.js renderer.
   *
   * @returns The initial playback time for media surface or standalone, or null otherwise.
   */
  const activeVideoInitialPlaybackTime = computed(() =>
    !isEntryDetailsMode.value || entryDetailsSurfaceMode.value === 'media'
      ? props.initialPlaybackTime
      : null,
  )

  /**
   * Indicates whether the active source follows a language or player selection change.
   *
   * @returns True when its preceding playback position and state should be restored.
   */
  const activeVideoPreservePlaybackOnSourceSwitch = computed(() =>
    activeVideoSource.value?.src === sourcePreservingPlayback.value,
  )

  /**
   * Indicates whether the episode autoplay toggle should be shown.
   *
   * @returns True when the media surface is active and episode autoplay toggle is enabled.
   */
  const activeVideoShowEpisodeAutoplayToggle = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.showEpisodeAutoplayToggle,
  )

  /**
   * Indicates whether episode autoplay is currently enabled.
   *
   * @returns True when the media surface is active and episode autoplay is enabled.
   */
  const activeVideoIsEpisodeAutoplayEnabled = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.isEpisodeAutoplayEnabled,
  )

  /**
   * Indicates whether video navigation controls should be shown in the player control bar.
   *
   * @returns True when the media surface is active and navigation controls are enabled.
   */
  const activeVideoShowVideoNavigationControls = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.showVideoNavigationControls,
  )

  /**
   * Indicates whether there is a previous video available to navigate to.
   *
   * @returns True when there is a previous playable video.
   */
  const activeVideoHasPreviousVideo = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.hasPreviousVideo,
  )

  /**
   * Indicates whether there is a next video available to navigate to.
   *
   * @returns True when there is a next playable video.
   */
  const activeVideoHasNextVideo = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.hasNextVideo,
  )

   /**
   * Indicates whether the big play button should be displayed on the Video.js player.
   * Controls visibility based on the controls prop, with CSS handling loading state.
   *
   * @returns True when the big play button should be shown.
   */
   const activeVideoShowBigPlayButton = computed(() => activeVideoControls.value)

   /**
   * Exposes the classes applied to the unified Video.js renderer.
   *
   * @returns CSS class for the video element.
   */
   const activeVideoClass = computed(() =>
     isEntryDetailsMode.value
       ? 'entry-details__trailer-frame entry-details__trailer-frame--video'
       : 'video-player__video--standalone',
   )

  /**
   * Indicates whether the entry-details picker should stay visible below the active media surface.
   *
   * @returns True when the picker should be shown.
   */
  const shouldShowDetailsPlayerPicker = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && showDetailsPlayerControls.value,
  )

  /**
   * Indicates whether the active Video.js renderer should forward progress and lifecycle events.
   *
   * @returns True when events should be forwarded.
   */
  const shouldForwardPrimaryVideoEvents = computed(() =>
    !isEntryDetailsMode.value || entryDetailsSurfaceMode.value === 'media',
  )

/**
 * Exposes a stable language key to the native select element.
 *
 * @returns The active language key or the first available language.
 */
const languageModel = computed(() =>
  props.activeLanguageKey ?? props.availableLanguages?.[0]?.key ?? '',
)

/**
 * Exposes a stable player identifier to the native select element.
 *
 * @returns The active player ID or the first available player.
 */
const playerModel = computed(() => props.activePlayerId ?? props.filteredPlayers?.[0]?.id ?? '')

/**
 * Stores the latest Video.js UI state so it can be restored after a source switch.
 *
 * @param value - Latest captured player state, or `null` when unavailable.
 */
const handlePlayerStateUpdate = (value: VideoJsPlayerState | null): void => {
  persistedVideoPlayerState.value = value

  if (value && shouldUseStoredPlayerPreferences.value) {
    storage.setVideoPlayerPreferences(createVideoPlayerPreferences(value))
  }
}

/**
 * Stores the newly selected language in the parent controller.
 *
 * @param event - Native select change event.
 */
const handleLanguageChange = (event: Event): void => {
  const target = event.target as HTMLSelectElement
  const nextLanguageKey = target.value || null

  if (nextLanguageKey !== languageModel.value) {
    isPlayerSelectionChangePending.value = true
  }

  emit('update:active-language-key', nextLanguageKey)
  emit('remember-current-language')
}

/**
 * Stores the newly selected player in the parent controller.
 *
 * @param event - Native select change event.
 */
const handlePlayerChange = (event: Event): void => {
  const target = event.target as HTMLSelectElement
  const nextPlayerId = target.value || null

  if (nextPlayerId !== playerModel.value) {
    isPlayerSelectionChangePending.value = true
  }

  emit('update:active-player-id', nextPlayerId)
  emit('remember-current-player')
}

/**
 * Forwards the current Video.js playback position to the entry details controller.
 *
 * @param value - Playback position in seconds, or `null` when it should be cleared.
 */
const handlePlaybackProgressUpdate = (value: number | null): void => {
  emit('update:playback-progress', value)
}

/**
 * Forwards the effective playback start notification to the parent controller.
 *
 * @param sourceUrl - The URL of the source that started playback.
 */
const handlePlaybackStarted = (sourceUrl: string | null): void => {
  emit('playback-started', sourceUrl)
}

/**
 * Forwards the playback-ended notification to the parent controller.
 */
const handlePlaybackEnded = (): void => {
  emit('playback-ended')
}

/**
 * Forwards the episode autoplay preference update emitted by the embedded Video.js control bar.
 *
 * @param value - Indicates whether automatic playback of the next episode is enabled.
 */
const handleEpisodeAutoplayEnabledUpdate = (value: boolean): void => {
  emit('update:is-episode-autoplay-enabled', value)
}

/**
 * Forwards the active Video.js playback progress only when the current surface is a primary video.
 *
 * @param value - Playback position in seconds, or `null` when it should be cleared.
 */
const handleActiveVideoPlaybackProgressUpdate = (value: number | null): void => {
  if (!shouldForwardPrimaryVideoEvents.value) {
    return
  }

  handlePlaybackProgressUpdate(value)
}

/**
 * Forwards the active Video.js playback-started notification only when it belongs to the primary video.
 *
 * @param sourceUrl - The URL of the source that started playback.
 */
const handleActiveVideoPlaybackStarted = (sourceUrl: string | null): void => {
  if (!shouldForwardPrimaryVideoEvents.value) {
    return
  }

  handlePlaybackStarted(sourceUrl)
}

/**
 * Forwards the active Video.js playback-ended notification only when it belongs to the primary video.
 */
const handleActiveVideoPlaybackEnded = (): void => {
  if (!shouldForwardPrimaryVideoEvents.value) {
    return
  }

  handlePlaybackEnded()
}

/**
 * Forwards the episode autoplay preference update only when the primary media surface is active.
 *
 * @param value - Indicates whether automatic playback of the next episode is enabled.
 */
const handleActiveVideoEpisodeAutoplayEnabledUpdate = (value: boolean): void => {
  if (entryDetailsSurfaceMode.value !== 'media') {
    return
  }

  handleEpisodeAutoplayEnabledUpdate(value)
}

/**
 * Forwards the video navigation event only when the primary media surface is active.
 *
 * @param direction - Navigation direction: -1 for previous, 1 for next.
 */
const handleActiveVideoVideoNavigation = (direction: -1 | 1): void => {
  if (entryDetailsSurfaceMode.value !== 'media') {
    return
  }

  emit('navigate-video', direction)
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
      activeVideoPreservePlaybackOnSourceSwitch,
      activeVideoShowEpisodeAutoplayToggle,
     activeVideoIsEpisodeAutoplayEnabled,
     activeVideoShowVideoNavigationControls,
     activeVideoHasPreviousVideo,
     activeVideoHasNextVideo,
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
     handleActiveVideoVideoNavigation,
   }
}
