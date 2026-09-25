import type { Collection, MediaItem } from '@/types/media'

/**
 * YAML-defined source descriptor used to resolve one category payload.
 */
export type HomeCategorySource = Record<string, string>

/**
 * Clickable category displayed in the home catalog.
 */
export interface HomeCategory {
  /** The unique identifier for the category. */
  id: string
  /** The display label for the category. */
  label: string
  /** The URL for the category image. */
  imageUrl: string | null
  /** The key used to merge categories with the same identifier. */
  mergeKey: string
  /** The list of source descriptors for this category. */
  sources: HomeCategorySource[]
}

/**
 * Player resolver descriptor embedded in a featured banner.
 *
 * When `videoUrl` is null, the banner can call `get_stream` with this descriptor
 * to obtain a playable stream URL on demand.
 */
export interface HomeBannerPlayer {
  /** The kind of resolver to use, e.g. `"rtbf-auvio-video"`. */
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
 * Featured banner rendered at the top of the home catalog.
 */
export interface HomeBanner {
  /** The unique identifier for the banner. */
  id: string
  /** The title to display on the banner. */
  title: string | null
  /** The subtitle to display on the banner. */
  subtitle: string | null
  /** The description to display on the banner. */
  description: string | null
  /** The URL for the background image. */
  imageUrl: string | null
  /** The URL for the logo image. */
  logoUrl: string | null
  /** The URL for the background video, or null when the video requires player resolution. */
  videoUrl: string | null
  /** Optional player resolver used to obtain a playable stream via `get_stream`. */
  player: HomeBannerPlayer | null
  /** The source identifier for the banner content. */
  source: string | null
  /** The internal API URL to fetch entry details. */
  entryUrl: string | null
  /** The public web URL to access the content. */
  webUrl: string | null
}

/**
 * Source descriptor used to lazily load or paginate one home section.
 */
export interface HomeSectionSource {
  /** The name of the source. */
  name: string
  /** The link to fetch content from this source. */
  link: string
  /** The current page number for pagination. */
  currentPage: number
  /** Whether there are more pages available. */
  haveMore: boolean
  /** Whether the first page for this source was included in `load_home`. */
  hasInitialItems: boolean
  /** Additional parameters for the source request. */
  params: Record<string, string>
}

/**
 * One selectable subsection within a grouped home media rail.
 */
export interface HomeSectionSubsection {
  /** Stable identifier used to merge the same subsection across sources. */
  key: string
  /** The display label for the subsection. */
  label: string | null
  /** The list of media items in this subsection. */
  items: MediaItem[]
  /** Source order used to interleave media items in this subsection. */
  sourceOrder: string[]
  /** The list of source descriptors for this subsection. */
  sources: HomeSectionSource[]
  /** The current page number for pagination. */
  currentPage: number
  /** Whether there are more pages available. */
  haveMore: boolean
}

/**
 * One grouped media rail rendered below the featured content.
 */
export interface HomeSection {
  /** The unique identifier for the section. */
  id: string
  /** The display label for the section. */
  label: string | null
  /**
   * Stable preference key used to persist frontend ordering choices.
   */
  preferenceKey: string
  /** The list of media items in this section. */
  items: MediaItem[]
  /** Source order used to interleave media items in this section. */
  sourceOrder: string[]
  /** The list of source descriptors for this section. */
  sources: HomeSectionSource[]
  /** The current page number for pagination. */
  currentPage: number
  /** Whether there are more pages available. */
  haveMore: boolean
  /** Optional selectable rails nested below this parent section. */
  subsections?: HomeSectionSubsection[]
}

/**
 * Full normalized payload rendered by the home and category screens.
 */
export interface HomeCatalogData {
  /** The featured banners collection, eagerly loaded or deferred by source. */
  banners: Collection<HomeBanner>
  /** Deferred banner collections, each retaining its backend source and link. */
  deferredBanners: Collection<HomeBanner>[]
  /** The list of clickable categories. */
  categories: HomeCategory[]
  /** The list of media sections to display. */
  sections: HomeSection[]
}
