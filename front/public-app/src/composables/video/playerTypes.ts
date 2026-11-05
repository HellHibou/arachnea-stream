import type { MediaItem } from '@/types/media'
import type { VideoJsMediaDimensions } from '@/composables/video/useVideoJsMediaRenderer'
import type { EntryPlayer } from '@/types/entry'
import type { EntryPlayerLanguageOption } from '@/composables/entry-details/entryVideoPlayer'
import type { ResolvedPlayerMediaSource } from '@/services/players'

/** Referrer policy options for iframe elements. */
export type MediaIframeReferrerPolicy =
  | 'no-referrer'
  | 'no-referrer-when-downgrade'
  | 'origin'
  | 'origin-when-cross-origin'
  | 'same-origin'
  | 'strict-origin'
  | 'strict-origin-when-cross-origin'
  | 'unsafe-url'

/** Entry-details surface currently occupying the player area. */
export type EntryDetailsSurfaceMode = 'media' | 'trailer'

/** Surface mode including standalone media. */
export type ActiveSurfaceMode = EntryDetailsSurfaceMode | 'standalone'

/**
 * Props accepted by the video player composable.
 */
export interface VideoPlayerProps {
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
  /** Title displayed when hovering the previous-video control. */
  previousVideoTitle?: string | null
  /** Title displayed when hovering the next-video control. */
  nextVideoTitle?: string | null
  /**
   * Mode used to render embedded iframe players.
   * @default 'safe'
   */
  securityMode?: 'unsafe' | 'confirmation' | 'safe'
}

/**
 * Emits accepted by the video player composable.
 */
export interface VideoPlayerEmits {
  /** Emitted when the active language key changes. */
  (evt: 'update:active-language-key', value: string | null): void
  /** Emitted when the current language should be remembered for future use. */
  (evt: 'remember-current-language'): void
  /** Emitted when the active player identifier changes. */
  (evt: 'update:active-player-id', value: string | null): void
  /** Emitted when the current player should be remembered for future use. */
  (evt: 'remember-current-player'): void
  /** Emitted when playback progress updates. */
  (evt: 'update:playback-progress', value: number | null, duration: number | null): void
  /** Emitted when the episode autoplay enabled state changes. */
  (evt: 'update:is-episode-autoplay-enabled', value: boolean): void
  /** Emitted when playback starts. */
  (evt: 'playback-started', sourceUrl: string | null): void
  /** Emitted when playback ends. */
  (evt: 'playback-ended'): void
  /** Emitted when video navigation is requested via control bar buttons. */
  (evt: 'navigate-video', direction: -1 | 1): void
}

/** Public component props before array defaults are applied by VideoPlayer.vue. */
export interface VideoPlayerComponentProps extends Omit<VideoPlayerProps, 'availableLanguages' | 'filteredPlayers'> {
  /**
   * Language options rendered by the entry details player selector.
   * @default []
   */
  availableLanguages?: EntryPlayerLanguageOption[]
  /**
   * Player options rendered by the entry details player selector.
   * @default []
   */
  filteredPlayers?: EntryPlayer[]
  /** Media item displayed when hovering the previous-video control. */
  previousVideoItem?: MediaItem | null
  /** Media item displayed when hovering the next-video control. */
  nextVideoItem?: MediaItem | null
  /** Service title of the entry. */
  serviceTitle?: string | null
}

/** Events exposed by the unified video player component. */
export interface VideoPlayerComponentEmits {
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
}
