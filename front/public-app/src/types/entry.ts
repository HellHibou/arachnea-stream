import type { Collection } from '@/types/media'

/**
 * Normalized playable item consumed by the featured media detail components.
 */
export interface EntryPlayableItem {
  /** The unique identifier for the playable item. */
  id: string
  /** The link to fetch additional details for this item. */
  link: string | null
  /** The collection of available players for this item. */
  players: Collection<EntryPlayer>
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
  /** Normalized paid-access indicator, or null when no paid offer applies. */
  price: string | null
  /** The URL for the preview of this item. */
  previewUrl: string | null
}

/**
 * Backward-compatible alias kept for program-specific episode flows.
 */
export type EntryEpisode = EntryPlayableItem

/**
 * Descriptor for a player that requires backend resolution to obtain a playable stream.
 *
 * `resolver` is a globally unique identifier for the resolution strategy (e.g. `stream-resolver`,
 * `m6play-video`, `tf1-video`). `target` is the value expected by that resolver (a URL, a media
 * identifier, etc.).
 */
export interface EntryPlayerResolver {
  /** The kind of resolver to use. */
  kind: string
  /** The target identifier to resolve. */
  targetId: string
  /** Optional source owning a fixed YAML `resolve_stream` query. */
  source?: string
  /** Optional ISO country hint used to route the resolver request through a geo proxy. */
  proxyCountry?: string
  /**
   * When enabled, absolute URLs inside proxied HLS manifests are rewritten to the
   * proxy path with the current proxy options, so child playlists and segments
   * keep the same routing (country, headers).
   */
  proxyRewriteManifestUrls?: boolean
}

/**
 * Resolver descriptor for an entry trailer that is resolved on demand.
 */
export type EntryTrailerResolver = EntryPlayerResolver

/**
 * Sprite thumbnail metadata exposed by one backend player.
 */
export interface EntryPlayerStoryboard {
  /** The URL to the sprite thumbnail image. */
  url: string
  /** The width of each thumbnail in the sprite, inferred from the image when omitted. */
  width?: number
  /** The height of each thumbnail in the sprite, inferred from the image when omitted. */
  height?: number
  /** The number of columns in the sprite image. */
  columns: number
  /** The number of thumbnail rows in each sprite image. */
  rows: number
  /** Index used for the first sprite image in a sequential URL template. */
  firstPageIndex: number
  /** The time interval between thumbnails in seconds, or `null` when derived from video duration. */
  interval: number | null
}

/**
 * A single chapter entry within a resolved player stream.
 */
export interface EntryPlayerChapter {
  /** Start time of the chapter in seconds. */
  start: number
  /** End time of the chapter in seconds. */
  end: number
  /** Display title of the chapter. */
  title: string
  /** Type discriminator for the chapter (e.g. "chapter"). */
  type: string
}

/**
 * A subtitle track returned by a resolved player stream.
 */
export interface ResolvedPlayerSubtitle {
  /** Optional language code for the subtitle track. */
  lang?: string
  /** Optional display label for the subtitle track. */
  label?: string
  /** Browser-consumable WebVTT URL. */
  link: string
}

/**
 * Resolved media stream returned by the backend `get_stream` command.
 *
 * This is one branch of the exclusive union `GetStreamResponse`:
 * either a direct media payload with `streamUrl[]` or an iframe fallback with `embedLink`.
 */
export interface EntryResolvedPlayerStream {
  /** Ordered list of alternative stream URLs (primary first). */
  streamUrl: string[]
  /** The type of the manifest (e.g., 'dash', 'hls'). */
  manifestType: string
  /** Optional title image URL returned by the selected player resolver. */
  imageTitleLink: string | null
  /** The URL to the DRM license server, or null for unprotected streams. */
  licenseUrl: string | null
  /** Headers to include when requesting the license. */
  licenseHeaders: Record<string, string>
  /** Optional WebVTT URL used for storyboard preview thumbnails. */
  storyboardVttUrl: string | null
  /** Optional sprite storyboard metadata. */
  storyboard: EntryPlayerStoryboard | null
  /** Optional ordered list of chapters extracted from the player metadata. */
  chapters: EntryPlayerChapter[] | null
  /** Subtitle tracks available for the resolved media stream. */
  subtitles: ResolvedPlayerSubtitle[]
}

/**
 * Fallback iframe response returned by `get_stream` when no YAML resolver matched the URL.
 *
 * The frontend renders this as an embed player, not as a video element.
 */
export interface EntryEmbedFallback {
  /** The iframe embed URL to render. */
  embedLink: string
}

/**
 * Union type returned by the `get_stream` backend command.
 *
 * - When `streamUrl[]` is present, the frontend uses the native video player with the first
 *   URL as primary and subsequent URLs as fallbacks on playback failure.
 * - When `embedLink` is present (exclusive), the frontend renders an iframe.
 * - The two branches are mutually exclusive.
 */
export type GetStreamResponse = EntryResolvedPlayerStream | EntryEmbedFallback

/**
 * Normalized embedded player entry consumed by the featured media detail component.
 */
export interface EntryPlayer {
  /** The unique identifier for the player. */
  id: string
  /** The display label for the player. */
  label: string
  /** The direct URL to the media. */
  directLink: string | null
  /** The public web URL opened externally when the direct link is not suitable. */
  webLink: string | null
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
  /** Optional resolver used to retrieve an ephemeral trailer stream on demand. */
  trailerResolver: EntryTrailerResolver | null
  /** The collection of available players for this entry. */
  players: Collection<EntryPlayer>
  /** The description of the entry. */
  description: string | null
  /** The URL for the poster image. */
  imagePosterUrl: string | null
  /** The URL for the portrait-oriented thumbnail. */
  imagePortraitUrl: string | null
  /** The URL for the landscape-oriented thumbnail. */
  imageLandscapeUrl: string | null
  /** Generic image URL used when no oriented image is available. */
  imageUrl: string | null
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
  /** Normalized paid-access indicator, or null when no paid offer applies. */
  price: string | null
}
