import type { MediaItem } from '@/types/media'

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
  /** The URL for the background video. */
  videoUrl: string | null
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
}

/**
 * Full normalized payload rendered by the home and category screens.
 */
export interface HomeCatalogData {
  /** The list of featured banners to display. */
  banners: HomeBanner[]
  /** The list of clickable categories. */
  categories: HomeCategory[]
  /** The list of media sections to display. */
  sections: HomeSection[]
}
