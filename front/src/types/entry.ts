/**
 * Normalized playable item consumed by the featured media detail components.
 */
export interface EntryPlayableItem {
  /** The unique identifier for the playable item. */
  id: string
  /** The link to fetch additional details for this item. */
  link: string | null
  /** The list of available players for this item. */
  players: EntryPlayer[]
  /** The name of the season this item belongs to. */
  seasonName: string | null
  /** The title of the playable item. */
  title: string | null
  /** The alternative title label for the item. */
  alternativeTitleLabel?: string | null
  /** The description of the playable item. */
  description: string | null
  /** The display label for the release date. */
  releaseDateLabel: string | null
  /** The display label for the expiration date. */
  expireLabel: string | null
  /** The display label for the duration. */
  durationLabel: string | null
  /** The URL for the preview of this item. */
  previewUrl: string | null
}

/**
 * Backward-compatible alias kept for program-specific episode flows.
 */
export type EntryEpisode = EntryPlayableItem

/**
 * Normalized embedded player entry consumed by the featured media detail component.
 */
export interface EntryPlayerResolver {
  /** The kind of resolver to use. */
  kind: string
  /** The target identifier to resolve. */
  targetId: string
  /** The kind of stream to resolve. */
  streamKind: string | null
}

/**
 * Sprite thumbnail metadata exposed by one backend player.
 */
export interface EntryPlayerStoryboard {
  /** The URL to the sprite thumbnail image. */
  url: string
  /** The width of each thumbnail in the sprite. */
  width: number
  /** The height of each thumbnail in the sprite. */
  height: number
  /** The number of columns in the sprite image. */
  columns: number
  /** The time interval between thumbnails in seconds. */
  interval: number
}

/**
 * Resolved protected or direct stream returned by the backend player resolver.
 */
export interface EntryResolvedPlayerStream {
  /** The URL to the stream manifest or media file. */
  streamUrl: string
  /** The type of the manifest (e.g., 'dash', 'hls'). */
  manifestType: string
  /** The URL to the DRM license server, or null for unprotected streams. */
  licenseUrl: string | null
  /** Headers to include when requesting the license. */
  licenseHeaders: Record<string, string>
}

/**
 * Normalized embedded player entry consumed by the featured media detail component.
 */
export interface EntryPlayer {
  /** The unique identifier for the player. */
  id: string
  /** The display label for the player. */
  label: string
  /** The URL to embed this player. */
  embedLink: string | null
  /** The direct URL to the media. */
  directLink: string | null
  /** The name of the player. */
  name: string | null
  /** The language code for this player's content. */
  lang: string | null
  /** The resolver configuration for this player. */
  resolver: EntryPlayerResolver | null
  /** The storyboard configuration for this player. */
  storyboard: EntryPlayerStoryboard | null
}

/**
 * Normalized paged season payload consumed by the entry details component.
 */
export interface EntryEpisodePage {
  /** The current page number. */
  currentPage: number
  /** Whether there are more pages available. */
  haveMore: boolean
  /** The episodes on the current page. */
  episodes: EntryEpisode[]
}

/**
 * Normalized season entry consumed by the featured media detail component.
 *
 * A season can provide episodes eagerly (embedded in `episodes[]`) or lazily
 * (via `link` for dynamic loading through `get_season`). When both are present,
 * the frontend prefers the embedded episodes.
 */
export interface EntrySeason {
  /** The unique identifier for the season. */
  id: string
  /** The display label for the season. */
  label: string | null
  /** The link to fetch season details dynamically. */
  link: string | null
  /** The list of episodes in this season. */
  episodes: EntryEpisode[]
}

/**
 * Normalized detailed media entry returned by the backend `get_entry` query.
 */
export interface EntryDetails {
  /** The source identifier for the entry. */
  source: string
  /** The internal API URL to fetch entry details. */
  entryUrl: string | null
  /** The primary title of the entry. */
  title: string | null
  /** The alternative title label for the entry. */
  alternativeTitleLabel: string | null
  /** The URL to the trailer for this entry. */
  trailerUrl: string | null
  /** The list of available players for this entry. */
  players: EntryPlayer[]
  /** The description of the entry. */
  description: string | null
  /** The URL for the poster image. */
  imagePosterUrl: string | null
  /** The URL for the portrait-oriented thumbnail. */
  imagePortraitUrl: string | null
  /** The URL for the landscape-oriented thumbnail. */
  imageLandscapeUrl: string | null
  /** The URL for the logo image. */
  logoUrl: string | null
  /** The URL for the hero banner image. */
  heroImageUrl: string | null
  /** The display label for the year. */
  yearLabel: string | null
  /** The display label for the release date. */
  releaseDateLabel: string | null
  /** The display label for the expiration date. */
  expireLabel: string | null
  /** The display label for the duration. */
  durationLabel: string | null
  /** The display label for the season count. */
  seasonCountLabel: string | null
  /** The display label for the audio language. */
  audioLanguageLabel: string | null
  /** The display label for the subtitle language. */
  subtitleLanguageLabel: string | null
  /** The content advisor rating label. */
  contentAdvisorLabel: string | null
  /** Labels for the themes associated with this entry. */
  themeLabels: string[]
  /** Labels for the genres associated with this entry. */
  genreLabels: string[]
  /** Labels for the cast members. */
  castingLabels: string[]
  /** Labels for the directors. */
  directorLabels: string[]
  /** The list of seasons for this entry. */
  seasons: EntrySeason[]
  /** The score or rating for this entry. */
  score: number | null
}