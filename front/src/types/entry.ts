/**
 * Normalized playable item consumed by the featured media detail components.
 */
export interface EntryPlayableItem {
  id: string
  link: string | null
  players: EntryPlayer[]
  seasonName: string | null
  title: string | null
  alternativeTitleLabel?: string | null
  description: string | null
  releaseDateLabel: string | null
  expireLabel: string | null
  durationLabel: string | null
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
  kind: string
  targetId: string
  streamKind: string | null
}

/**
 * Sprite thumbnail metadata exposed by one backend player.
 */
export interface EntryPlayerStoryboard {
  url: string
  width: number
  height: number
  columns: number
  interval: number
}

/**
 * Resolved protected or direct stream returned by the backend player resolver.
 */
export interface EntryResolvedPlayerStream {
  streamUrl: string
  manifestType: string
  licenseUrl: string | null
  licenseHeaders: Record<string, string>
}

/**
 * Normalized embedded player entry consumed by the featured media detail component.
 */
export interface EntryPlayer {
  id: string
  label: string
  embedLink: string | null
  directLink: string | null
  name: string | null
  lang: string | null
  resolver: EntryPlayerResolver | null
  storyboard: EntryPlayerStoryboard | null
}

/**
 * Normalized paged season payload consumed by the entry details component.
 */
export interface EntryEpisodePage {
  currentPage: number
  haveMore: boolean
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
  id: string
  label: string | null
  link: string | null
  episodes: EntryEpisode[]
}

/**
 * Normalized detailed media entry returned by the backend `get_entry` query.
 */
export interface EntryDetails {
  source: string
  entryUrl: string | null
  title: string | null
  alternativeTitleLabel: string | null
  trailerUrl: string | null
  players: EntryPlayer[]
  description: string | null
  imagePosterUrl: string | null
  imagePortraitUrl: string | null
  imageLandscapeUrl: string | null
  logoUrl: string | null
  heroImageUrl: string | null
  yearLabel: string | null
  releaseDateLabel: string | null
  expireLabel: string | null
  durationLabel: string | null
  seasonCountLabel: string | null
  audioLanguageLabel: string | null
  subtitleLanguageLabel: string | null
  contentAdvisorLabel: string | null
  themeLabels: string[]
  genreLabels: string[]
  castingLabels: string[]
  directorLabels: string[]
  seasons: EntrySeason[]
  score: number | null
}