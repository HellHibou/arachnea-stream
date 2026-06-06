import type { MediaItem } from '@/types/media'

/**
 * YAML-defined source descriptor used to resolve one category payload.
 */
export type HomeCategorySource = Record<string, string>

/**
 * Clickable category displayed in the home catalog.
 */
export interface HomeCategory {
  id: string
  label: string
  imageUrl: string | null
  description: string | null
  mergeKey: string
  sources: HomeCategorySource[]
}

/**
 * Featured banner rendered at the top of the home catalog.
 */
export interface HomeBanner {
  id: string
  title: string | null
  subtitle: string | null
  description: string | null
  imageUrl: string | null
  logoUrl: string | null
  videoUrl: string | null
  source: string | null
  entryUrl: string | null
  webUrl: string | null
}

/**
 * Source descriptor used to lazily load or paginate one home section.
 */
export interface HomeSectionSource {
  name: string
  link: string
  currentPage: number
  haveMore: boolean
  params: Record<string, string>
}

/**
 * One grouped media rail rendered below the featured content.
 */
export interface HomeSection {
  id: string
  label: string | null
  /**
   * Stable preference key used to persist frontend ordering choices.
   */
  preferenceKey: string
  items: MediaItem[]
  sources: HomeSectionSource[]
  currentPage: number
  haveMore: boolean
}

/**
 * Full normalized payload rendered by the home and category screens.
 */
export interface HomeCatalogData {
  banners: HomeBanner[]
  categories: HomeCategory[]
  sections: HomeSection[]
}
