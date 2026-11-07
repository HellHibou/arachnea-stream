import type { ResolvedPlayerMediaSource } from '@/services/players'
import type { HomeCategory } from '@/types/home'

/** Explicit live target, independent of membership in the live channel list. */
export interface LiveSelectionTarget {
  /** Backend source that resolves the target. */
  source: string
  /** Opaque identifier accepted by get_live, including event identifiers. */
  channel: string
  /** Optional display title retained when opening a direct link. */
  title?: string | null
  /** Optional catalog artwork retained when opening a direct link. */
  imageUrl?: string | null
  /** Optional catalog description retained when opening a direct link. */
  description?: string | null
  /** Optional public page of the selected live content. */
  webUrl?: string | null
}

/** Catalog cards share presentation and retain source-specific navigation descriptors. */
export type CatalogItem = MediaItem

/**
 * Determines the effective thumbnail orientation for a media item based on
 * available images, falling back to the default when both types are present
 * or when neither is available.
 *
 * @param item Media item with available image URLs.
 * @param defaultOrientation Fallback orientation when both or neither type is available.
 * @returns The effective orientation for this item.
 */
export function getEffectiveOrientation(
  item: CatalogItem,
  defaultOrientation: ThumbnailOrientation,
): ThumbnailOrientation {
  const hasPortrait = Boolean(item.imagePosterUrl ?? item.imagePortraitUrl)
  const hasLandscape = Boolean(item.imageLandscapeUrl)

  if (hasPortrait && !hasLandscape) return 'portrait'
  if (hasLandscape && !hasPortrait) return 'landscape'
  return defaultOrientation
}

/**
 * Supported aspect ratios for media thumbnails.
 */
export type ThumbnailOrientation = 'portrait' | 'landscape'

/**
 * Supported poster fitting strategies inside a media card.
 */
export type ThumbnailImageFit = 'cover' | 'contain'

/**
 * Raw media candidate that can be rendered behind the page.
 */
export interface BackgroundMediaCandidate {
  /** URL of the background image, or null if not available. */
  imageUrl: string | null
  /** URL of the background video, or null if not available. */
  videoUrl: string | null
  /** Fully resolved background video source, including optional DRM configuration. */
  videoSource?: ResolvedPlayerMediaSource | null
}

/**
 * Supported layouts for the media card collection component.
 */
export type MediaCardCollectionMode = 'grid' | 'single-row' | 'list'

/**
 * Content target routed by its media types and source-owned descriptors.
 */
export interface MediaSelectionTarget {
  /** Canonical media types used by the shared routing registry; absent for legacy targets. */
  mediaTypeValues?: string[]
  /** Source-owned request parameters retained for registered and future destinations. */
  request?: Record<string, string>
  /** Resolved category request descriptor, derived from the source payload. */
  category?: HomeCategory
  /** Resolved opaque live target, derived from the source payload. */
  live?: LiveSelectionTarget

  /** The source identifier for the media entry. */
  source: string | null
  /** Legacy entry URL or live-list opaque link; specialized descriptors take precedence. */
  entryUrl: string | null
  /** The public web URL to access the entry. */
  webUrl: string | null
  /**
   * The display title of the entry, used for the informative route slug.
   *
   * Optional because bare selection targets may omit it; concrete media items
   * always provide it through `MediaItem`.
   */
  title?: string | null
}

/**
 * Normalized media entry consumed by the frontend catalog components.
 */
export interface MediaItem extends MediaSelectionTarget {
  /** The unique identifier for the media item. */
  id: string
  /** The primary title of the media item. */
  title: string | null
  /** Alternative title label for the media item. */
  alternativeTitleLabel: string | null
  /** URL for the poster image of the media item. */
  imagePosterUrl: string | null
  /** URL for the portrait-oriented thumbnail image. */
  imagePortraitUrl: string | null
  /** URL for the landscape-oriented thumbnail image. */
  imageLandscapeUrl: string | null
  /** Generic thumbnail URL used when no oriented image is available. */
  imageUrl: string | null
  /** Display label for the media type. */
  mediaTypeLabel: string | null
  /** Raw media type values for categorization. */
  mediaTypeValues: string[]
  /** Labels for the themes associated with this media. */
  themeLabels: string[]
  /** Display label for the audio track. */
  audioLabel: string | null
  /** Display label for the duration. */
  durationLabel: string | null
  /** Numeric rating for the media item. */
  rating: number | null
  /** Summary or description of the media content. */
  overview: string | null
  /** Display label for the episode information. */
  episodeLabel: string | null
  /** Display label for the release date. */
  releaseDateLabel: string | null
  /** Display label for the expiration date. */
  expireLabel: string | null
  /** Normalized paid-access indicator, or null when no paid offer applies. */
  price: string | null
  /** Additional metadata line to display. */
  metaLine: string | null
}

/**
 * Supported persisted text-track display modes.
 */
export type VideoJsTextTrackModePreference = 'showing' | 'disabled'

/**
 * Minimal media track identity used to restore audio and subtitle choices.
 */
export interface VideoJsTrackPreference {
  /** The unique identifier for the track. */
  id: string | null
  /** The language code for the track. */
  language: string | null
  /** The display label for the track. */
  label: string | null
  /** The kind of track (e.g., 'audio', 'subtitles'). */
  kind: string | null
}

/**
 * Persisted subtitle track preference, including whether subtitles were disabled.
 */
export interface VideoJsTextTrackPreference extends VideoJsTrackPreference {
  /** The display mode preference for text tracks. */
  mode: VideoJsTextTrackModePreference
}

/**
 * Audio language targeted by one ordered Video.js track profile.
 *
 * `original` selects the main non-audio-description track, `null` keeps the
 * audio selection untouched, and any other value is a language code.
 */
export type VideoJsTrackProfileAudio = string | 'original' | null

/**
 * Subtitle behaviour applied once one Video.js track profile is retained.
 */
export type VideoJsTrackProfileSubtitles = 'disabled' | 'auto'

/**
 * Ordered audio and subtitle combination applied to a video source.
 */
export interface VideoJsTrackProfile {
  /** Audio language searched in the source, or `original` for the main track. */
  audio: VideoJsTrackProfileAudio
  /** Subtitle behaviour applied after the audio track is selected. */
  subtitles: VideoJsTrackProfileSubtitles
}

/**
 * Visual text-track settings exposed by Video.js.
 */
export type VideoJsTextTrackSettings = Partial<Record<
  | 'backgroundColor'
  | 'backgroundOpacity'
  | 'color'
  | 'edgeStyle'
  | 'fontFamily'
  | 'fontPercent'
  | 'textOpacity'
  | 'windowColor'
  | 'windowOpacity',
  string | number
>>

/**
 * Persistent Video.js session state restored when the integrated player switches source.
 */

/**
 * Descriptor for a deferred-loaded collection following the hybrid contract.
 *
 * When `link` is present, the frontend should call the backend query identified by `source`
 * with that `link` to obtain the remaining items. When `link` is absent, all items are
 * already present in `entries`.
 */
export interface Collection<T> {
  /** Items already loaded in the current payload. */
  entries: T[]
  /** YAML service name that can load the deferred items. */
  source: string
  /** Internal link to pass to the backend when deferred loading is needed. */
  link?: string
}
export interface VideoJsPlayerState {
  /** The current volume level (0.0 to 1.0). */
  volume: number
  /** Whether the player is muted. */
  muted: boolean
  /** The current playback rate. */
  playbackRate: number
  /** The current quality for the video source. */
  quality: string | null
  /** The selected audio track preference. */
  audioTrack: VideoJsTrackPreference | null
  /** The selected text track preference. */
  textTrack: VideoJsTextTrackPreference
  /** The current text track display settings. */
  textTrackSettings: VideoJsTextTrackSettings | null
  /** Whether the player is in fullscreen mode. */
  isFullscreen: boolean
  /** Whether to show remaining time instead of elapsed time. */
  showsRemainingTime: boolean
}
