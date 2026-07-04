import videojs from 'video.js'
import type { ShallowRef } from 'vue'

import type { ResolvedVideoMediaSource } from '@/services/players'
import type { VideoJsPlayerState } from '@/types/media'

/**
 * Natural dimensions exposed by one loaded video source.
 */
export interface VideoJsMediaDimensions {
  width: number
  height: number
  aspectRatio: number
}

/**
 * Props accepted by the shared Video.js renderer.
 */
export interface VideoJsMediaRendererProps {
  /**
   * Resolved media source rendered through the integrated Video.js player.
   */
  source: ResolvedVideoMediaSource
  /**
   * Accessible label applied to the interactive video element.
   * @default null
   */
  videoAriaLabel?: string | null
  /**
   * Optional poster image used before playback starts.
   * @default null
   */
  poster?: string | null
  /**
   * Optional logo overlaid on top of the poster before playback starts.
   * @default null
   */
  overlayLogoUrl?: string | null
  /**
   * Enables autoplay when supported by the browser.
   * @default false
   */
  autoplay?: boolean
  /**
   * Mutes the rendered video.
   * @default false
   */
  muted?: boolean
  /**
   * Loops the rendered video.
   * @default false
   */
  loop?: boolean
  /**
   * Displays player controls.
   * @default false
   */
  controls?: boolean
  /**
   * Enables inline playback on mobile browsers.
   * @default false
   */
  playsinline?: boolean
  /**
   * Preload strategy applied to the player.
   * @default 'metadata'
   */
  preload?: 'none' | 'metadata' | 'auto'
  /**
   * Hides decorative media from accessibility APIs.
   * @default false
   */
  ariaHidden?: boolean
  /**
   * Optional tabindex forwarded to the underlying video element.
   * @default null
   */
  tabIndex?: number | null
  /**
   * Playback position restored after the player has loaded its metadata.
   * @default null
   */
  initialPlaybackTime?: number | null
  /**
   * Indicates whether the episode autoplay toggle should be mounted inside the Video.js control bar.
   * @default false
   */
  showEpisodeAutoplayToggle?: boolean
  /**
   * Indicates whether automatic playback of the next episode is currently enabled.
   * @default false
   */
  isEpisodeAutoplayEnabled?: boolean
  /**
   * Previously captured Video.js UI state restored after remounting the renderer.
   * @default null
   */
  initialPlayerState?: VideoJsPlayerState | null
  /**
   * Keeps the big play button hidden while the parent surface resolves a new media source.
   * @default false
   */
  isExternalLoading?: boolean
  /**
   * Controls visibility of the big play button overlay.
   * When undefined, defaults to the value of the `controls` prop.
   */
  showBigPlayButton?: boolean
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
 * Emits accepted by the shared Video.js renderer.
 */
export interface VideoJsMediaRendererEmits {
  (evt: 'update:playback-progress', value: number | null): void
  (evt: 'playback-started', sourceUrl: string | null): void
  (evt: 'playback-ended'): void
  (evt: 'update:is-episode-autoplay-enabled', value: boolean): void
  (evt: 'update:player-state', value: VideoJsPlayerState | null): void
  (evt: 'video-initial-load-complete'): void
  (evt: 'video-metadata-loaded', value: VideoJsMediaDimensions): void
  (evt: 'navigate-video', direction: -1 | 1): void
}

/**
 * Readonly shallow ref to a DOM element or null.
 */
export type TemplateElementRef<T extends Element> = Readonly<ShallowRef<T | null>>

/**
 * Options for the useVideoJsMediaRenderer composable.
 */
export interface UseVideoJsMediaRendererOptions {
  /**
   * Props passed to the Video.js media renderer.
   */
  props: VideoJsMediaRendererProps
  /**
   * Emitted events from the Video.js media renderer.
   */
  emit: VideoJsMediaRendererEmits
  /**
   * Template reference to the host DOM element.
   */
  hostElement: TemplateElementRef<HTMLDivElement>
  /**
   * Template reference to the video DOM element.
   */
  videoElement: TemplateElementRef<HTMLVideoElement>
}

/**
 * Extended Video.js player type with additional Arachnea-specific methods and properties.
 */
export type VideoJsPlayer = ReturnType<typeof videojs> & {
  /**
   * Gets the audio track list handle.
   * @returns The audio track list handle.
   */
  audioTracks?: () => VideoJsAudioTrackListHandle
  /**
   * Handler for document fullscreen change events.
   * @param event - The fullscreen change event.
   */
  documentFullscreenChange_?: (event: Event) => void
  /**
   * Initializes Encrypted Media Extensions.
   * @param options - EME options.
   */
  eme?: (options?: unknown) => void
  /**
   * Exits picture-in-picture mode.
   * @returns Promise that resolves when exited.
   */
  exitPictureInPicture?: () => Promise<unknown> | unknown
  /**
   * Initializes hotkeys.
   * @param options - Hotkey configuration options.
   */
  hotkeys?: (options?: unknown) => void
  /**
   * Whether the player is currently in picture-in-picture mode.
   * @returns True when in picture-in-picture mode.
   */
  isInPictureInPicture?: () => boolean
  /**
   * Gets the quality level list.
   * @returns Quality level list.
   */
  qualityLevels?: () => unknown
  /**
   * Initializes the quality menu.
   * @param options - Quality menu configuration options.
   */
  qualityMenu?: (options?: {
    /** Default resolution label. */
    defaultResolution?: string
    /** Bitrate limit for SD quality. */
    sdBitrateLimit?: number
    /** Whether to use resolution labels. */
    useResolutionLabels?: boolean
    /** Whether to show bitrates in resolution labels. */
    resolutionLabelBitrates?: boolean
  }) => void
  /**
   * Initializes sprite thumbnails.
   * @param options - Sprite thumbnail configuration.
   */
  spriteThumbnails?: (options?: Record<string, unknown>) => void
  /**
   * Gets the text track list handle.
   * @returns The text track list handle.
   */
  textTracks?: () => VideoJsTextTrackListHandle
  /**
   * Text track settings handle.
   */
  textTrackSettings?: VideoJsTextTrackSettingsHandle
  /**
   * Live stream tracker for live playback.
   */
  liveTracker?: {
    /**
     * Gets the seekable time ranges.
     * @returns Seekable time ranges or null.
     */
    seekable?: () => TimeRanges | null
    /**
     * Gets the current live time.
     * @returns Current live time in seconds.
     */
    liveCurrentTime?: () => number
  }
}

/**
 * Handle to a single Video.js audio track.
 */
export type VideoJsAudioTrackHandle = {
  /** Whether the track is currently enabled. */
  enabled?: boolean
  /** Unique identifier for the track. */
  id?: string
  /** Kind of the track (e.g., main, alternative). */
  kind?: string
  /** Human-readable label for the track. */
  label?: string
  /** Language of the track. */
  language?: string
}

/**
 * Handle to the Video.js audio track list.
 */
export type VideoJsAudioTrackListHandle = {
  /** Number of audio tracks in the list. */
  readonly length: number
  /** Index signature to access individual tracks. */
  [index: number]: VideoJsAudioTrackHandle
  /**
   * Adds an event listener for change events.
   * @param eventName - Event name to listen for.
   * @param listener - Event listener callback.
   */
  addEventListener?: (eventName: 'change', listener: (event: Event) => void) => void
  /**
   * Removes an event listener for change events.
   * @param eventName - Event name to remove.
   * @param listener - Event listener callback to remove.
   */
  removeEventListener?: (eventName: 'change', listener: (event: Event) => void) => void
}

/**
 * Handle to a single Video.js text track.
 */
export type VideoJsTextTrackHandle = {
  /** Unique identifier for the track. */
  id?: string
  /** Kind of the track (e.g., subtitles, captions). */
  kind?: string
  /** Human-readable label for the track. */
  label?: string
  /** Language of the track. */
  language?: string
  /** Mode of the track (e.g., showing, hidden). */
  mode?: string
}

/**
 * Handle to the Video.js text track list.
 */
export type VideoJsTextTrackListHandle = {
  /** Number of text tracks in the list. */
  readonly length: number
  /** Index signature to access individual tracks. */
  [index: number]: VideoJsTextTrackHandle
  /**
   * Adds an event listener for change events.
   * @param eventName - Event name to listen for.
   * @param listener - Event listener callback.
   */
  addEventListener?: (eventName: 'change', listener: (event: Event) => void) => void
  /**
   * Removes an event listener for change events.
   * @param eventName - Event name to remove.
   * @param listener - Event listener callback to remove.
   */
  removeEventListener?: (eventName: 'change', listener: (event: Event) => void) => void
}

/**
 * Handle to Video.js text track settings.
 */
export type VideoJsTextTrackSettingsHandle = {
  /**
   * Gets the current settings values.
   * @returns Record mapping setting names to values.
   */
  getValues?: () => Record<string, unknown>
  /**
   * Sets new settings values.
   * @param values - Settings values to apply.
   */
  setValues?: (values: Record<string, unknown>) => void
  /** Updates the display of text track settings. */
  updateDisplay?: () => void
}

/**
 * Handle to Video.js menu button events.
 */
export type VideoJsMenuButtonHandle = {
  /**
   * Removes an event listener.
   * @param eventName - Name of the event to remove.
   */
  off: (eventName: string) => void
}

/**
 * Component type for Video.js menu buttons.
 */
export type VideoJsMenuButtonComponent = {
  /** Internal menu button handle. */
  menuButton_?: VideoJsMenuButtonHandle
}

/**
 * Component type for Video.js quality menu buttons.
 */
export type QualityMenuButtonComponent = VideoJsMenuButtonComponent & {
  /** List of quality menu items. */
  items?: QualityMenuItemHandle[]
}

/**
 * Handle to a single quality menu item.
 */
export type QualityMenuItemHandle = {
  /** Internal options for the menu item. */
  options_?: {
    /** Display label for the quality option. */
    label?: string
  }
  /** List of quality levels associated with this item. */
  levels_?: number[]
  /** Whether the item is currently selected. */
  selected_?: boolean
  /**
   * Checks if the item has a specific CSS class.
   * @param className - CSS class name to check.
   * @returns True when the class is present.
   */
  hasClass?: (className: string) => boolean
  /** Handles click events on the menu item. */
  handleClick?: () => void
  /**
   * Sets the selected state of the item.
   * @param active - Whether the item should be selected.
   */
  selected?: (active: boolean) => void
}

/**
 * Handle to the Video.js quality level list.
 */
export type QualityLevelListHandle = {
  /** Index of the currently selected quality level. */
  selectedIndex: number
}

/**
 * VHS (Video.js HTTP Streaming) playlist configuration.
 */
export type VhsPlaylist = {
  /** Unique identifier for the playlist. */
  id?: string
  /** URI of the playlist. */
  uri?: string
  /** Whether the playlist is disabled. */
  disabled?: boolean
  /** Timestamp until which the playlist should be excluded. */
  excludeUntil?: number
  /** Playlist attributes. */
  attributes?: {
    /** Bitrate of the playlist in bits per second. */
    BANDWIDTH?: number
    /** Resolution of the playlist. */
    RESOLUTION?: {
      /** Width of the resolution. */
      width?: number
      /** Height of the resolution. */
      height?: number
    }
  }
}

/**
 * Handle to VHS (Video.js HTTP Streaming) handler.
 */
export type VhsHandlerHandle = {
  /** VHS playlists configuration. */
  playlists?: {
    /** Main playlist configuration. */
    main?: {
      /** Array of available VHS playlists. */
      playlists?: VhsPlaylist[]
    }
  }
  /** Current estimated system bandwidth in bits per second. */
  systemBandwidth?: number
  /**
   * Selects the appropriate playlist based on current conditions.
   * @returns The selected playlist or null.
   */
  selectPlaylist: () => VhsPlaylist | null
}

/**
 * Handle to Video.js tech (playback technology) instance.
 */
export type VideoJsTechHandle = {
  /** VHS handler for HTTP streaming. */
  vhs?: VhsHandlerHandle
}

/**
 * Input configuration for Video.js media source.
 */
export type VideoJsSourceInput = {
  /** The URL to the media source. */
  src: string
  /** MIME type of the media source. */
  type?: string
  /** Key systems for encrypted content. */
  keySystems?: Record<string, unknown>
  /** Configuration for sprite thumbnails. */
  spriteThumbnails?: Record<string, unknown>
}

/**
 * Current active source in the Video.js player.
 */
export type VideoJsCurrentSource = {
  /** The URL of the currently active source. */
  src?: string
}

/**
 * Document type extended with cross-browser fullscreen API methods and properties.
 */
export type FullscreenDocument = Document & {
  /** Current element in fullscreen mode (WebKit prefix). */
  webkitFullscreenElement?: Element | null
  /** Current element in fullscreen mode (Mozilla prefix). */
  mozFullScreenElement?: Element | null
  /** Current element in fullscreen mode (Microsoft prefix). */
  msFullscreenElement?: Element | null
  /** Method to exit fullscreen mode (WebKit prefix). */
  webkitExitFullscreen?: () => Promise<unknown> | unknown
  /** Method to exit fullscreen mode (Microsoft prefix). */
  msExitFullscreen?: () => Promise<unknown> | unknown
}

/**
 * Host element type extended with cross-browser fullscreen API methods.
 */
export type FullscreenHostElement = HTMLDivElement & {
  /** Method to request fullscreen mode (WebKit prefix). */
  webkitRequestFullscreen?: () => Promise<unknown> | unknown
  /** Method to request fullscreen mode (Microsoft prefix). */
  msRequestFullscreen?: () => Promise<unknown> | unknown
}

/**
 * Promise-like object with catch method for error handling.
 */
export type PromiseLikeWithCatch<T = unknown> = PromiseLike<T> & {
  /**
   * Handles rejection of the promise.
   * @param onRejected - Callback function for rejection.
   * @returns New promise-like object.
   */
  catch: (onRejected?: (reason: unknown) => unknown) => PromiseLike<T>
}
