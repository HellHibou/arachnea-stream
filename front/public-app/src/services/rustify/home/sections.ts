import { buildHomeContentIdentity } from '@/services/homeContentIdentity'
import type { CatalogItem } from '@/types/media'
import type {
  HomeCatalogData,
  HomeSection,
  HomeSectionSource,
  HomeSectionSubsection,
} from '@/types/home'

import { call_api } from '../apiClient'
import { dedupeMediaItems, normalizeCatalogItems } from '../media'
import { dedupeDisplayStrings } from '../primitives/formatting'
import { interleaveLists } from '../primitives/lists'
import {
  buildScopedId,
  firstNonEmptyString,
  firstNumber,
  isJsonRecord,
  normalizeString,
  readBoolean,
  readRecordList,
  readStringParamMap,
  slugify,
} from '../primitives/readers'
import type { JsonRecord } from '../types'

/**
 * Loads one additional page for a lazily loaded home section.
 *
 * @param section Section descriptor previously returned by `loadHomeCatalog`.
 * @param sources Source descriptors to load for the section.
 * @param strict Reject source failures instead of silently retaining their cursors (default false).
 * @returns Normalized section payload assembled from every fulfilled source response.
 */
export async function loadHomeSectionPage(
  section: HomeSection,
  sources: HomeSectionSource[],
  strict = false,
): Promise<HomeSection> {
  if (sources.length === 0) {
    return section
  }

  const isInitialLoad = sources.some((source) => !source.hasInitialItems)
  const settledSources = await Promise.allSettled(sources.map(async (source) => {
    const page = source.hasInitialItems ? source.currentPage + 1 : 1
    const response = await call_api<unknown>('get_section', {
      source: source.name,
      link: source.link,
      page,
      sourceParams: [{
        source: source.name,
        link: source.link,
        ...source.params,
        page: String(page),
      }],
    })
    const responseRecord: JsonRecord = isJsonRecord(response) ? response : {}
    const responseSection = readRecordList(responseRecord.sections)[0] ?? responseRecord
    const normalizedSection = normalizeHomeSection(
      {
        ...responseSection,
        label: section.label,
        entries: readRecordList(responseSection.entries),
        current_page: responseSection.current_page ?? page,
        have_more: responseSection.have_more ?? false,
      },
      0,
      source.name,
    )

    return { source, normalizedSection }
  }))
  if (strict) {
    const failure = settledSources.find((result) => result.status === 'rejected')
    if (failure?.status === 'rejected') throw failure.reason
  }
  const loadedSources = settledSources
    .filter((result): result is PromiseFulfilledResult<{
      source: HomeSectionSource
      normalizedSection: HomeSection
    }> => result.status === 'fulfilled')
    .map((result) => result.value)
  const loadedSourcesByKey = new Map(
    loadedSources.map(({ source, normalizedSection }) => [
      buildHomeSectionSourceKey(source),
      normalizedSection,
    ]),
  )

  return {
    ...section,
    items: isInitialLoad
      ? interleaveInitialSectionItems(section, loadedSources)
      : dedupeMediaItems([
          ...section.items,
          ...interleaveLists(
            loadedSources.map(({ normalizedSection }) => normalizedSection.items),
          ),
        ]),
    currentPage: Math.max(
      section.currentPage,
      ...loadedSources.map(({ normalizedSection }) => normalizedSection.currentPage),
    ),
    haveMore: loadedSources.some(({ normalizedSection }) => normalizedSection.haveMore) ||
      section.sources.some((source) =>
        !loadedSourcesByKey.has(buildHomeSectionSourceKey(source)) && source.haveMore,
      ),
    sources: section.sources.map((source) => {
      const normalizedSection = loadedSourcesByKey.get(buildHomeSectionSourceKey(source))
      if (!normalizedSection) {
        return source
      }

      return {
        ...source,
        currentPage: normalizedSection.currentPage,
        haveMore: normalizedSection.haveMore,
        hasInitialItems: true,
      }
    }),
  }
}

/**
 * Loads every deferred first page before sections are rendered and interleaved.
 *
 * @param catalog Normalized catalog returned by the home or category endpoint.
 * @returns Catalog whose sections include every available initial source page.
 */
export async function loadInitialHomeSectionPages(
  catalog: HomeCatalogData,
): Promise<HomeCatalogData> {
  const sections = await Promise.all(catalog.sections.map((section) => {
    const initialSources = section.sources.filter((source) => !source.hasInitialItems)
    return loadHomeSectionPage(section, initialSources)
  }))

  return { ...catalog, sections }
}

/**
 * Converts one raw backend section into the frontend grouped collection model.
 *
 * @param payload Raw backend section payload.
 * @param index Stable fallback index used to build a unique section id.
 * @returns Normalized home section.
 */
export function normalizeHomeSection(
  payload: unknown,
  index: number,
  inheritedSource: string | null = null,
): HomeSection {
  const record = isJsonRecord(payload) ? payload : {}
  const rawItems = readRecordList(record.entries)
  const rawSubsections = readRecordList(record.subsections)
  const label = firstNonEmptyString([record.label])
  const currentPage = Math.max(1, Math.trunc(firstNumber(record.current_page) ?? 1))
  const haveMore = readBoolean(record.have_more)
  const sources = normalizeHomeSectionSources(
    record,
    inheritedSource,
    currentPage,
    haveMore,
    rawItems.length > 0,
  )
  const id =
    buildScopedId(
      firstNonEmptyString([record.id, record.key]) ?? (slugify(label ?? '') || `section-${index + 1}`),
      inheritedSource,
    ) ??
    `section-${index + 1}`

  return {
    id,
    label,
    identity: buildHomeContentIdentity(
      inheritedSource,
      firstNonEmptyString([record.key, record.id]),
      { ...readStringParamMap(record.request), ...readStringParamMap({ link: record.link }) },
      label,
    ),
    preferenceKey: buildHomeSectionPreferenceKey(label, id),
    items: normalizeCatalogItems(rawItems, inheritedSource),
    sourceOrder: [inheritedSource ?? id],
    sources,
    currentPage,
    haveMore,
    subsections: rawSubsections.length > 0
      ? rawSubsections.map((subsection, subsectionIndex) =>
          normalizeHomeSectionSubsection(subsection, subsectionIndex, inheritedSource),
        )
      : undefined,
  }
}

/**
 * Converts one raw backend subsection into a selectable media rail.
 *
 * @param payload Raw backend subsection payload.
 * @param index Stable fallback index used to build a subsection key.
 * @param inheritedSource Source inherited from the parent scraper row.
 * @returns Normalized home subsection.
 */
function normalizeHomeSectionSubsection(
  payload: unknown,
  index: number,
  inheritedSource: string | null = null,
): HomeSectionSubsection {
  const record = isJsonRecord(payload) ? payload : {}
  const rawItems = readRecordList(record.entries)
  const label = firstNonEmptyString([record.label])
  const currentPage = Math.max(1, Math.trunc(firstNumber(record.current_page) ?? 1))
  const haveMore = readBoolean(record.have_more)
  const key = firstNonEmptyString([record.key, record.id]) ??
    (slugify(label ?? '') || `subsection-${index + 1}`)

  return {
    key,
    label,
    identity: buildHomeContentIdentity(
      inheritedSource,
      firstNonEmptyString([record.key, record.id]),
      { ...readStringParamMap(record.request), ...readStringParamMap({ link: record.link }) },
      label,
    ),
    items: normalizeCatalogItems(rawItems, inheritedSource),
    sourceOrder: [inheritedSource ?? key],
    sources: normalizeHomeSectionSources(record, inheritedSource, currentPage, haveMore, rawItems.length > 0),
    currentPage,
    haveMore,
  }
}

/**
 * Converts one raw backend section link into frontend lazy-loading source descriptors.
 *
 * @param record Raw backend section payload.
 * @param inheritedSource Source inherited from the parent scraper row.
 * @param currentPage Current loaded page exposed by the backend.
 * @param haveMore Whether the backend reports more items for this section.
 * @param hasInitialItems Whether the initial page contains items from this source.
 * @returns Section source descriptors usable by `get_section`.
 */
function normalizeHomeSectionSources(
  record: JsonRecord,
  inheritedSource: string | null,
  currentPage: number,
  haveMore: boolean,
  hasInitialItems: boolean,
): HomeSectionSource[] {
  const link = firstNonEmptyString([record.link])

  if (!link || !inheritedSource) {
    return []
  }

  return [
    {
      name: inheritedSource,
      link,
      currentPage,
      haveMore,
      hasInitialItems,
      params: readStringParamMap(record.request),
    },
  ]
}

/**
 * Merges section rails that share the same visible label.
 *
 * @param sections Normalized section rails collected from one or more sources.
 * @returns Aggregated section rails with deduplicated media items.
 */
export function mergeHomeSections(sections: HomeSection[]): HomeSection[] {
  const sectionsByPreferenceKey = new Map<string, HomeSection>()
  const itemListsByPreferenceKey = new Map<string, CatalogItem[][]>()
  const subsectionsByPreferenceKey = new Map<string, HomeSectionSubsection[]>()
  const mergedSections: HomeSection[] = []

  const sectionsBySource = new Map<string, HomeSection[]>()
  sections.forEach((section) => {
    const sourceKey = section.sourceOrder[0] ?? section.id
    const sourceSections = sectionsBySource.get(sourceKey) ?? []
    sourceSections.push(section)
    sectionsBySource.set(sourceKey, sourceSections)
  })

  interleaveLists([...sectionsBySource.values()]).forEach((section) => {
    const existingSection = sectionsByPreferenceKey.get(section.preferenceKey)

    if (!existingSection) {
      const mergedSection: HomeSection = {
        ...section,
        items: dedupeMediaItems(section.items),
        sourceOrder: [...section.sourceOrder],
        sources: dedupeHomeSectionSources(section.sources),
      }

      sectionsByPreferenceKey.set(section.preferenceKey, mergedSection)
      itemListsByPreferenceKey.set(section.preferenceKey, [section.items])
      subsectionsByPreferenceKey.set(section.preferenceKey, section.subsections ?? [])
      mergedSections.push(mergedSection)
      return
    }

    existingSection.label ??= section.label
    itemListsByPreferenceKey.get(section.preferenceKey)?.push(section.items)
    subsectionsByPreferenceKey.get(section.preferenceKey)?.push(...(section.subsections ?? []))
    existingSection.sourceOrder = dedupeDisplayStrings([
      ...existingSection.sourceOrder,
      ...section.sourceOrder,
    ])
    existingSection.sources = dedupeHomeSectionSources([
      ...existingSection.sources,
      ...section.sources,
    ])
    existingSection.currentPage = Math.max(existingSection.currentPage, section.currentPage)
    existingSection.haveMore = existingSection.haveMore || section.haveMore
  })

  mergedSections.forEach((section) => {
    section.items = dedupeMediaItems(
      interleaveLists(itemListsByPreferenceKey.get(section.preferenceKey) ?? []),
    )
    const subsections = mergeHomeSectionSubsections(
      subsectionsByPreferenceKey.get(section.preferenceKey) ?? [],
    )
    section.subsections = subsections.length > 0 ? subsections : undefined
  })

  return mergedSections
}

/**
 * Merges subsections by stable key without combining separate time periods.
 *
 * @param subsections Normalized subsections collected for one parent section.
 * @returns Selectable rails with period-specific entries retained.
 */
function mergeHomeSectionSubsections(subsections: HomeSectionSubsection[]): HomeSectionSubsection[] {
  const subsectionsByKey = new Map<string, HomeSectionSubsection>()
  const itemListsByKey = new Map<string, CatalogItem[][]>()
  const mergedSubsections: HomeSectionSubsection[] = []

  subsections.forEach((subsection) => {
    const existingSubsection = subsectionsByKey.get(subsection.key)
    if (!existingSubsection) {
      const mergedSubsection: HomeSectionSubsection = {
        ...subsection,
        items: dedupeMediaItems(subsection.items),
        sourceOrder: [...subsection.sourceOrder],
        sources: dedupeHomeSectionSources(subsection.sources),
      }
      subsectionsByKey.set(subsection.key, mergedSubsection)
      itemListsByKey.set(subsection.key, [subsection.items])
      mergedSubsections.push(mergedSubsection)
      return
    }

    existingSubsection.label ??= subsection.label
    itemListsByKey.get(subsection.key)?.push(subsection.items)
    existingSubsection.sourceOrder = dedupeDisplayStrings([
      ...existingSubsection.sourceOrder,
      ...subsection.sourceOrder,
    ])
    existingSubsection.sources = dedupeHomeSectionSources([
      ...existingSubsection.sources,
      ...subsection.sources,
    ])
    existingSubsection.currentPage = Math.max(existingSubsection.currentPage, subsection.currentPage)
    existingSubsection.haveMore = existingSubsection.haveMore || subsection.haveMore
  })

  mergedSubsections.forEach((subsection) => {
    subsection.items = dedupeMediaItems(
      interleaveLists(itemListsByKey.get(subsection.key) ?? []),
    )
  })

  return mergedSubsections
}

/**
 * Interleaves all initial source items, including deferred pages loaded before rendering.
 *
 * @param section Merged section before its deferred initial sources are loaded.
 * @param loadedSources Results returned by the deferred sources.
 * @returns Deduplicated initial section items in source round-robin order.
 */
function interleaveInitialSectionItems(
  section: HomeSection,
  loadedSources: Array<{ source: HomeSectionSource, normalizedSection: HomeSection }>,
): CatalogItem[] {
  const itemsBySource = new Map<string, CatalogItem[]>()
  const sourceOrder = [...section.sourceOrder]

  section.items.forEach((item) => {
    const sourceKey = item.source ?? item.id
    if (!itemsBySource.has(sourceKey)) {
      itemsBySource.set(sourceKey, [])
      sourceOrder.push(sourceKey)
    }

    itemsBySource.get(sourceKey)?.push(item)
  })

  loadedSources.forEach(({ source, normalizedSection }) => {
    if (!itemsBySource.has(source.name)) {
      itemsBySource.set(source.name, [])
      sourceOrder.push(source.name)
    }

    itemsBySource.get(source.name)?.push(...normalizedSection.items)
  })

  return dedupeMediaItems(
    interleaveLists(sourceOrder.map((source) => itemsBySource.get(source) ?? [])),
  )
}

/**
 * Deduplicates section source descriptors while preserving their first-seen order.
 *
 * @param sources Source descriptors attached to one merged section.
 * @returns Deduplicated section source descriptors.
 */
function dedupeHomeSectionSources(sources: HomeSectionSource[]): HomeSectionSource[] {
  const seen = new Set<string>()
  const dedupedSources: HomeSectionSource[] = []

  sources.forEach((source) => {
    const serializedSource = buildHomeSectionSourceKey(source)

    if (seen.has(serializedSource)) {
      return
    }

    seen.add(serializedSource)
    dedupedSources.push({ ...source, params: { ...source.params } })
  })

  return dedupedSources
}

/**
 * Builds the stable preference key used to collapse equivalent section rails.
 *
 * @param label Visible section label when available.
 * @param id Unique section identifier used as fallback for unlabeled rails.
 * @returns Case-insensitive label key, or a unique fallback for unlabeled rails.
 */
function buildHomeSectionPreferenceKey(label: string | null, id: string): string {
  const normalizedLabel = normalizeString(label ?? '')
  return normalizedLabel ? `label:${normalizedLabel.toLocaleLowerCase()}` : `id:${id}`
}

/**
 * Builds a stable identity key for a section source descriptor.
 *
 * @param source Source descriptor attached to a home section.
 * @returns Source identity key excluding mutable pagination state.
 */
function buildHomeSectionSourceKey(source: HomeSectionSource): string {
  return JSON.stringify([
    source.name,
    source.link,
    Object.entries(source.params).sort(([left], [right]) => left.localeCompare(right)),
  ])
}
