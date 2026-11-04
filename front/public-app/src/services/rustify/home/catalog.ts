import { t } from '@/i18n'
import { markAmbiguousHomeIdentities } from '@/services/homeContentIdentity'
import type { HomeSourceDiscovery } from '@/types/customHome'
import type { Collection } from '@/types/media'
import type { HomeBanner, HomeCatalogData, HomeCategory, HomeSection } from '@/types/home'

import { call_api } from '../apiClient'
import {
  firstNonEmptyString,
  isJsonRecord,
  readBoolean,
  readPath,
  readRecordList,
} from '../primitives/readers'
import { normalizeHomeBanner, readBannerList } from './banners'
import { mergeHomeCategories, normalizeHomeCategory } from './categories'
import { mergeHomeSections, normalizeHomeSection } from './sections'

/**
 * Loads the aggregated home payload exposed by the backend and normalizes it for the UI.
 *
 * @param options Optional exact source selection; omitted means all, empty means none.
 * @returns Featured banners, clickable categories, and grouped catalog rails.
 */
export async function loadHomeCatalog(options: { sources?: string[] } = {}): Promise<HomeCatalogData> {
  const sources = options.sources === undefined ? undefined : [...new Set(options.sources)].sort()
  const response = await call_api<unknown>('load_home', { sources })

  return normalizeHomeCatalog(response)
}

/**
 * Discovers only selected sources, without merging or loading deferred content.
 * @param sources Exact backend source ids; empty selects nothing without a network call.
 * @returns Independent source catalogs retaining inline sections and identity diagnostics.
 */
export async function loadHomeSourceDiscovery(sources: string[]): Promise<HomeSourceDiscovery[]> {
  if (sources.some((source) => !source.trim() || source !== source.trim())) {
    throw new Error('Source discovery requires non-blank, exact source ids')
  }
  const selected = [...new Set(sources)].sort()
  if (selected.length === 0) return []
  const response = await call_api<unknown>('load_home', { sources: selected })
  if (!Array.isArray(response) && !isJsonRecord(response)) {
    throw new Error(t('errors.unexpectedHomeResponseFormat'))
  }
  const rows = readRecordList(response)
  return selected.map((sourceId) => {
    const sourceRows = rows.filter((row) =>
      firstNonEmptyString([row.source, readPath(row, 'source', 'name')]) === sourceId,
    )
    const catalog = normalizeHomeCatalog(sourceRows, false)
    markAmbiguousHomeIdentities(catalog.categories)
    markAmbiguousHomeIdentities(catalog.sections)
    for (const section of catalog.sections) {
      markAmbiguousHomeIdentities(section.subsections ?? [])
    }
    return { sourceId, available: sourceRows.length > 0, catalog }
  })
}

/**
 * Loads one aggregated category payload exposed by the backend and normalizes it for the UI.
 *
 * @param category Category descriptor previously returned by `loadHomeCatalog`.
 * @param page 1-based category page requested by the frontend.
 * @param merge Whether to apply legacy semantic merging (default true).
 * @returns Normalized banners, categories, and grouped catalog rails for the selected category.
 */
export async function getCategoryCatalog(category: HomeCategory, page = 1, merge = true): Promise<HomeCatalogData> {
  if (category.sources.length === 0) {
    throw new Error(t('errors.categoryMissingBackendSourceDescriptors'))
  }

  const response = await call_api<unknown>('get_category', {
    sources: category.sources,
    page,
    sourceParams: category.sources.map((source) => ({
      source: source.name,
      page: String(page),
    })),
  })

  const catalog = normalizeHomeCatalog(response, merge)
  const rows = readRecordList(response)
  return { ...catalog, currentPage: page, haveMore: rows.some((row) => readBoolean(row.have_more)) }
}

/**
 * Converts one raw backend home/category payload into the frontend home catalog model.
 *
 * @param payload Raw backend home/category payload.
 * @param merge Whether to preserve legacy semantic aggregation (default true).
 * @returns Normalized home catalog data ready for rendering.
 */
export function normalizeHomeCatalog(payload: unknown, merge = true): HomeCatalogData {
  if (!Array.isArray(payload) && !isJsonRecord(payload)) {
    throw new Error(t('errors.unexpectedHomeResponseFormat'))
  }

  const rows = readRecordList(payload)
  const banners: Collection<HomeBanner> = { entries: [], source: '' }
  const deferredBanners: Collection<HomeBanner>[] = []
  const categories: HomeCategory[] = []
  const sections: HomeSection[] = []

  rows.forEach((row) => {
    const rowSource = firstNonEmptyString([row.source, readPath(row, 'source', 'name')])
    const bannerCollection = isJsonRecord(row.banners) ? row.banners : null
    const bannerSource = firstNonEmptyString([bannerCollection?.source, rowSource])
    const bannerLink = firstNonEmptyString([bannerCollection?.link])

    const bannerEntries: HomeBanner[] = []
    readBannerList(bannerCollection?.entries ?? row.banners).forEach((banner) => {
      const normalizedBanner = normalizeHomeBanner(
        banner,
        banners.entries.length + bannerEntries.length,
        bannerSource,
      )
      if (
        normalizedBanner.entryUrl &&
        (
          normalizedBanner.imageUrl ||
          normalizedBanner.title ||
          normalizedBanner.videoUrl ||
          normalizedBanner.player
        )
      ) {
        bannerEntries.push(normalizedBanner)
      }
    })

    banners.entries.push(...bannerEntries)

    if (bannerSource && bannerLink) {
      deferredBanners.push({ entries: [], source: bannerSource, link: bannerLink })
    }

    readRecordList(row.categories).forEach((category) => {
      const normalizedCategory = normalizeHomeCategory(category, categories.length, rowSource)
      if (normalizedCategory.sources.length > 0) {
        categories.push(normalizedCategory)
      }
    })

    readRecordList(row.sections).forEach((section) => {
      const normalizedSection = normalizeHomeSection(section, sections.length, rowSource)
      if (
        normalizedSection.items.length > 0 ||
        normalizedSection.sources.length > 0 ||
        (!merge && normalizedSection.haveMore) ||
        (normalizedSection.subsections?.length ?? 0) > 0
      ) {
        sections.push(normalizedSection)
      }
    })
  })

  const catalog = { banners, deferredBanners, categories, sections }
  if (!merge) return catalog
  const aggregated = mergeNormalizedCatalogs([catalog])
  for (const category of aggregated.categories) delete category.identity
  for (const section of aggregated.sections) {
    delete section.identity
    for (const subsection of section.subsections ?? []) delete subsection.identity
  }
  return aggregated
}

/**
 * Merges multiple normalized catalogs into one display-ready payload.
 *
 * @param catalogs Normalized catalogs produced from one or more backend responses.
 * @returns Aggregated catalog with merged categories and merged section rails.
 */
function mergeNormalizedCatalogs(catalogs: HomeCatalogData[]): HomeCatalogData {
  const banners: Collection<HomeBanner> = {
    entries: catalogs.flatMap((catalog) => catalog.banners.entries),
    source: '',
  }
  const deferredBanners = catalogs.flatMap((catalog) => catalog.deferredBanners)
  const categories = mergeHomeCategories(catalogs.flatMap((catalog) => catalog.categories))
  const sections = mergeHomeSections(catalogs.flatMap((catalog) => catalog.sections))

  return {
    banners,
    deferredBanners,
    categories,
    sections,
  }
}
