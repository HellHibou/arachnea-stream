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
  imageUrl: string | null
  videoUrl: string | null
}

/**
 * Supported layouts for the media card collection component.
 */
export type MediaCardCollectionMode = 'grid' | 'single-row' | 'list'

/**
 * Minimal target required to open an entry details screen.
 */
export interface MediaSelectionTarget {
  source: string | null
  entryUrl: string | null
  webUrl: string | null
}

/**
 * Normalized media entry consumed by the frontend catalog components.
 */
export interface MediaItem extends MediaSelectionTarget {
  id: string
  title: string | null
  alternativeTitleLabel: string | null
  imagePosterUrl: string | null
  imagePortraitUrl: string | null
  imageLandscapeUrl: string | null
  mediaTypeLabel: string | null
  mediaTypeValues: string[]
  themeLabels: string[]
  audioLabel: string | null
  durationLabel: string | null
  rating: number | null
  overview: string | null
  episodeLabel: string | null
  releaseDateLabel: string | null
  expireLabel: string | null
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
  id: string | null
  language: string | null
  label: string | null
  kind: string | null
}

/**
 * Persisted subtitle track preference, including whether subtitles were disabled.
 */
export interface VideoJsTextTrackPreference extends VideoJsTrackPreference {
  mode: VideoJsTextTrackModePreference
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
export interface VideoJsPlayerState {
  volume: number
  muted: boolean
  playbackRate: number
  qualityLabel: string | null
  audioTrack: VideoJsTrackPreference | null
  textTrack: VideoJsTextTrackPreference
  textTrackSettings: VideoJsTextTrackSettings | null
  isFullscreen: boolean
  showsRemainingTime: boolean
}
