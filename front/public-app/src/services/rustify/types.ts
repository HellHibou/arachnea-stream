import type { MediaItem } from '@/types/media'

/** Supported media type values used for filtering. */
export const mediaTypeValues = [
  'collection',
  'video/movie',
  'video/show/serie',
  'video/show/anime',
  'video/show/documentary',
  'video/show/sport',
  'video/news',
  'video/show/other',
  'video/live',
  'images/manga',
  'images/webtoon',
  'audio/show',
  'audio/show/serie',
  'audio/other',
] as const

/**
 * Option for a search filter dropdown.
 */
export interface SearchFilterOption {
  /** The filter value to send to the backend. */
  value: string
  /** The display label for this filter option. */
  label: string
}

/** Type alias for a JSON object record with string keys and unknown values. */
export type JsonRecord = Record<string, unknown>

/**
 * Search filters sent to the backend catalog endpoint.
 */
export interface SearchMediaItemsFilters {
  mediaTypes?: string[]
  themes?: string[]
}

/**
 * Paginated search results page.
 */
export interface SearchMediaItemsPage {
  /** The media items on this page. */
  items: MediaItem[]
  /** The current page number. */
  currentPage: number
  /** Whether there are more pages available. */
  haveMore: boolean
  /** Source-specific parameters for fetching the next page. */
  sourceParams: Record<string, string>[]
}
