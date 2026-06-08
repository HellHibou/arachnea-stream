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
}

export type TemplateElementRef<T extends Element> = Readonly<ShallowRef<T | null>>

export interface UseVideoJsMediaRendererOptions {
  props: VideoJsMediaRendererProps
  emit: VideoJsMediaRendererEmits
  hostElement: TemplateElementRef<HTMLDivElement>
  videoElement: TemplateElementRef<HTMLVideoElement>
}

export type VideoJsPlayer = ReturnType<typeof videojs> & {
  audioTracks?: () => VideoJsAudioTrackListHandle
  documentFullscreenChange_?: (event: Event) => void
  eme?: (options?: unknown) => void
  exitPictureInPicture?: () => Promise<unknown> | unknown
  hotkeys?: (options?: unknown) => void
  isInPictureInPicture?: () => boolean
  qualityLevels?: () => unknown
  qualityMenu?: (options?: {
    defaultResolution?: string
    sdBitrateLimit?: number
    useResolutionLabels?: boolean
    resolutionLabelBitrates?: boolean
  }) => void
  spriteThumbnails?: (options?: Record<string, unknown>) => void
  textTracks?: () => VideoJsTextTrackListHandle
  textTrackSettings?: VideoJsTextTrackSettingsHandle
  liveTracker?: {
    seekable?: () => TimeRanges | null
    liveCurrentTime?: () => number
  }
}

export type VideoJsAudioTrackHandle = {
  enabled?: boolean
  id?: string
  kind?: string
  label?: string
  language?: string
}

export type VideoJsAudioTrackListHandle = {
  readonly length: number
  [index: number]: VideoJsAudioTrackHandle
  addEventListener?: (eventName: 'change', listener: (event: Event) => void) => void
  removeEventListener?: (eventName: 'change', listener: (event: Event) => void) => void
}

export type VideoJsTextTrackHandle = {
  id?: string
  kind?: string
  label?: string
  language?: string
  mode?: string
}

export type VideoJsTextTrackListHandle = {
  readonly length: number
  [index: number]: VideoJsTextTrackHandle
  addEventListener?: (eventName: 'change', listener: (event: Event) => void) => void
  removeEventListener?: (eventName: 'change', listener: (event: Event) => void) => void
}

export type VideoJsTextTrackSettingsHandle = {
  getValues?: () => Record<string, unknown>
  setValues?: (values: Record<string, unknown>) => void
  updateDisplay?: () => void
}

export type VideoJsMenuButtonHandle = {
  off: (eventName: string) => void
}

export type VideoJsMenuButtonComponent = {
  menuButton_?: VideoJsMenuButtonHandle
}

export type QualityMenuButtonComponent = VideoJsMenuButtonComponent & {
  items?: QualityMenuItemHandle[]
}

export type QualityMenuItemHandle = {
  options_?: {
    label?: string
  }
  levels_?: number[]
  selected_?: boolean
  hasClass?: (className: string) => boolean
  handleClick?: () => void
  selected?: (active: boolean) => void
}

export type QualityLevelListHandle = {
  selectedIndex: number
}

export type VhsPlaylist = {
  id?: string
  uri?: string
  disabled?: boolean
  excludeUntil?: number
  attributes?: {
    BANDWIDTH?: number
    RESOLUTION?: {
      width?: number
      height?: number
    }
  }
}

export type VhsHandlerHandle = {
  playlists?: {
    main?: {
      playlists?: VhsPlaylist[]
    }
  }
  systemBandwidth?: number
  selectPlaylist: () => VhsPlaylist | null
}

export type VideoJsTechHandle = {
  vhs?: VhsHandlerHandle
}

export type VideoJsSourceInput = {
  src: string
  type?: string
  keySystems?: Record<string, unknown>
  spriteThumbnails?: Record<string, unknown>
}

export type VideoJsCurrentSource = {
  src?: string
}

export type FullscreenDocument = Document & {
  webkitFullscreenElement?: Element | null
  mozFullScreenElement?: Element | null
  msFullscreenElement?: Element | null
  webkitExitFullscreen?: () => Promise<unknown> | unknown
  msExitFullscreen?: () => Promise<unknown> | unknown
}

export type FullscreenHostElement = HTMLDivElement & {
  webkitRequestFullscreen?: () => Promise<unknown> | unknown
  msRequestFullscreen?: () => Promise<unknown> | unknown
}

export type PromiseLikeWithCatch<T = unknown> = PromiseLike<T> & {
  catch: (onRejected?: (reason: unknown) => unknown) => PromiseLike<T>
}
