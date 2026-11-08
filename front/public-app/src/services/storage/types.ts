import type { Ref } from 'vue'

import type {
  MediaCardCollectionMode,
  ThumbnailImageFit,
  ThumbnailOrientation,
  VideoJsTextTrackPreference,
  VideoJsTextTrackSettings,
  VideoJsTrackPreference,
  VideoJsTrackProfile,
} from '@/types/media'

/**
 * Configuration entry for a storable parameter.
 */
export interface ParameterEntry {
  /** The parameter key used for storage. */
  key: string
  /** The default value for the parameter. */
  defaultValue: any
}

/**
 * Bookmark persisted for one entry details view.
 */
export interface EntryBookmark {
  /**
   * Selected group identifier to restore when reopening the entry.
   */
  groupId: string | null
  /**
   * Selected playable item identifier to restore when reopening the entry.
   */
  selectedItemId: string | null
  /**
   * Last known playback position in seconds for Video.js media.
   */
  playbackTime: number | null
  /**
   * Primary display title snapshot from `get_entry`.
   */
  title: string | null
  /**
   * Alternative title label snapshot from `get_entry`.
   */
  alternativeTitleLabel: string | null
  /**
   * Poster image URL snapshot from `get_entry`.
   */
  imagePosterUrl: string | null
  /**
   * Landscape image URL snapshot from `get_entry`.
   */
  imageLandscapeUrl: string | null
  /**
   * Generic image URL snapshot from `get_entry`, used when no oriented
   * poster or landscape image is available.
   */
  imageUrl: string | null
  /**
   * Description snapshot from `get_entry`.
   */
  description: string | null
  /**
   * Media type label snapshot from `get_entry`.
   */
  mediaTypeLabel: string | null
  /**
   * Display label of the currently selected season or group.
   */
  seasonLabel: string | null
  /**
   * Number of the current or last watched episode (1-based within the displayed
   * episode list), or `null` when outside an episode context.
   */
  episodeNumber: number | null
  /**
   * Whether the current episode reached its end and no episode follows it.
   */
  isFullyWatched: boolean
}

/**
 * Video.js preferences persisted across player instances.
 */
export interface VideoPlayerPreferences {
  /**
   * Last selected volume level from 0 to 1.
   */
  volume: number
  /**
   * Indicates whether the player was muted.
   */
  muted: boolean
  /**
   * Last manually selected quality, or `null` for Auto.
   */
  quality: string | null
  /**
   * Last selected audio track, or `null` when unavailable.
   */
  audioTrack: VideoJsTrackPreference | null
  /**
   * Last selected subtitle track, or disabled subtitle state.
   */
  textTrack: VideoJsTextTrackPreference
  /**
   * Last selected visual subtitle settings.
   */
  textTrackSettings: VideoJsTextTrackSettings | null
}

/**
 * Props accepted by the search display parameters component.
 */
export interface Parameters {
  /** Stable custom home id; null keeps the aggregated landing page. */
  defaultHomeId: Ref<string | null>
  /**
   * Thumbnail orientation applied to search results.
   */
  thumbnailOrientation: Ref<ThumbnailOrientation>
  /**
   * Poster fit mode applied to search results.
   */
  thumbnailImageFit: Ref<ThumbnailImageFit>
  /**
   * Size multiplier applied to media card thumbnails in grid and single-row modes.
   */
  thumbnailSizeMultiplier: Ref<number>
  /**
   * Layout mode applied to the result collection.
   */
  collectionMode: Ref<MediaCardCollectionMode>
  /**
   * Indicates whether the trailer should be used as the page background when available.
   */
  useTrailerAsBackground: Ref<boolean>
  /**
   * Indicates whether home/category banner and thumbnail images should be reused as the page background.
   */
  useCatalogBannersAsBackground: Ref<boolean>
  /**
   * Indicates whether the page background should animate.
   */
  isBackgroundAnimated: Ref<boolean>
  /**
   * Controls whether the background image should be fully visible or cropped.
   */
  backgroundImageFit: Ref<ThumbnailImageFit>
  /**
   * Indicates whether the integrated player should automatically start the next episode.
   */
  isEpisodeAutoplayEnabled: Ref<boolean>
  /**
   * Indicates whether the integrated player should automatically skip recap chapters.
   */
  'videoPlayer.autoskip.previously': Ref<boolean>
  /**
   * Indicates whether the integrated player should automatically skip intro chapters.
   */
  'videoPlayer.autoskip.intro': Ref<boolean>
  /**
   * Indicates whether the integrated player should automatically skip coming-next chapters.
   */
  'videoPlayer.autoskip.coming_next': Ref<boolean>
  /**
   * Indicates whether the integrated player should automatically skip outro chapters.
   */
  'videoPlayer.autoskip.outro': Ref<boolean>
  /**
   * Indicates whether the integrated player should automatically skip advertising chapters.
   */
  'videoPlayer.autoskip.ads': Ref<boolean>
  /**
   * Ordered audio and subtitle combinations applied to a new video source.
   */
  'videoPlayer.trackProfiles': Ref<VideoJsTrackProfile[]>
  /**
   * Subtitle languages applied by `auto` track profiles, highest priority first.
   */
  'videoPlayer.subtitleLanguagePriority': Ref<string[]>
  /**
   * Configured subtitle languages, highest priority first.
   */
  'videoPlayer.subtitleLanguages': Ref<string[]>
  /**
   * Indicates whether audio-description tracks may be selected as a last resort.
   */
  'videoPlayer.allowAudioDescription': Ref<boolean>
  /**
   * Mode used to render embedded iframe players.
   * - 'unsafe' : loads the iframe immediately.
   * - 'confirmation' : shows the episode image and a switch before loading the iframe.
   * - 'safe' : blocks embedded players and hides source buttons.
   */
  securityMode: Ref<'unsafe' | 'confirmation' | 'safe'>
  /**
   * Preferred interface language, or `null` to follow the browser language.
   */
  language: Ref<string | null>
  /**
   * Selected theme preset key.
   */
  theme: Ref<string>
}

/**
 * Display options applied when one section record is created.
 */
export interface SectionRecordDefaults {
  /** Initial collection mode for the new record. */
  collectionMode?: MediaCardCollectionMode
  /** Initial thumbnail orientation for the new record. */
  thumbnailOrientation?: ThumbnailOrientation
  /** Initial thumbnail image fit for the new record. */
  thumbnailImageFit?: ThumbnailImageFit
}

/**
 * Home catalog preferences persisted locally.
 */
export interface HomePreferences {
  /**
   * Ordered list of pinned home section keys.
   */
  pinnedSectionOrder: Ref<string[]>
  /**
   * Indicates whether home editing buttons should remain visible.
   */
  showSectionEditingButtons: Ref<boolean>
  /**
   * Per-section collection mode overrides.
   * Key: section preferenceKey, Value: collection mode
   */
  sectionCollectionMode: Ref<Record<string, MediaCardCollectionMode>>
  /**
   * Per-section thumbnail orientation overrides.
   * Key: section preferenceKey, Value: orientation
   */
  sectionThumbnailOrientation: Ref<Record<string, ThumbnailOrientation>>
  /**
   * Per-section thumbnail image fit overrides.
   * Key: section preferenceKey, Value: fit mode
   */
  sectionThumbnailImageFit: Ref<Record<string, ThumbnailImageFit>>
}
