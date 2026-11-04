import { t } from '@/i18n'
import type { MediaItem } from '@/types/media'

import { call_api } from './apiClient'
import { normalizeMediaItem } from './media'
import { interleaveLists } from './primitives/lists'
import {
  firstNonEmptyString,
  firstNumber,
  isJsonRecord,
  readOptionalBoolean,
  readPath,
  readRecordList,
  readStringParamMap,
  toPositiveInteger,
} from './primitives/readers'
import type {
  JsonRecord,
  SearchMediaItemsFilters,
  SearchMediaItemsPage,
} from './types'

/**
 * Executes the backend search endpoint and normalizes the response for the UI.
 *
 * @param query Raw search query sent to the Rust backend.
 * @param filters Optional media type and theme filters sent with the search request.
 * @returns Normalized media cards consumable by the frontend components.
 */
export async function searchMediaItems(
  query: string,
  filters: SearchMediaItemsFilters = {},
): Promise<MediaItem[]> {
  const page = await searchMediaItemsPage(query, filters)

  return page.items
}

/**
 * Executes one paged backend search and normalizes entries with pagination metadata.
 *
 * @param query Raw search query sent to the Rust backend.
 * @param filters Optional media type and theme filters sent with the search request.
 * @param page 1-based page requested by the frontend.
 * @param sourceParams Source-specific pagination params returned by the previous page.
 * @returns Normalized media cards and pagination metadata.
 */
export async function searchMediaItemsPage(
  query: string,
  filters: SearchMediaItemsFilters = {},
  page = 1,
  sourceParams: Record<string, string>[] = [],
): Promise<SearchMediaItemsPage> {
  const response = await call_api<unknown>('search', {
    query,
    page,
    mediaTypes: filters.mediaTypes ?? [],
    themes: filters.themes ?? [],
    sourceParams,
  })

  const groups = readSearchGroups(response)
  if (!groups) {
    throw new Error(t('errors.unexpectedSearchResponse'))
  }

  const nextSourceParams = buildSearchNextSourceParams(groups, page)
  const itemsBySource = groups.map((group) => {
    const source = firstNonEmptyString([group.source, readPath(group, 'source', 'name')])

    return readRecordList(group.entries).map((entry, entryIndex) =>
      normalizeMediaItem(entry, entryIndex, source),
    )
  })

  return {
    items: interleaveLists(itemsBySource),
    currentPage: Math.max(1, Math.trunc(page)),
    haveMore: nextSourceParams.length > 0,
    sourceParams: nextSourceParams,
  }
}

/**
 * Reads source-grouped search rows from the backend search response.
 *
 * The expected shape is one row per source, with source-level pagination fields and an `entries`
 * array containing the media rows.
 *
 * @param response Raw backend search response.
 * @returns Search source groups, or `null` when the payload shape is unsupported.
 */
function readSearchGroups(response: unknown): JsonRecord[] | null {
  if (Array.isArray(response)) {
    const rows = response.filter(isJsonRecord)
    return rows.some((row) => 'entries' in row) ? rows : groupFlatSearchEntries(rows)
  }

  if (isJsonRecord(response) && 'entries' in response) {
    return groupFlatSearchEntries(readRecordList(response.entries))
  }

  return null
}

/**
 * Groups legacy flat search entries by source.
 *
 * @param entries Flat search rows.
 * @returns Source-grouped search rows.
 */
function groupFlatSearchEntries(entries: JsonRecord[]): JsonRecord[] {
  const groupsBySource = new Map<string, JsonRecord>()
  const groups: JsonRecord[] = []

  entries.forEach((entry) => {
    const source = firstNonEmptyString([entry.source, readPath(entry, 'source', 'name')]) ?? ''
    let group = groupsBySource.get(source)

    if (!group) {
      group = source ? { source, entries: [] } : { entries: [] }
      groupsBySource.set(source, group)
      groups.push(group)
    }

    const item = { ...entry }
    delete item.source
    const sourceGroupFields = [
      'current_page',
      'have_more',
      'total_pages',
      'next_value',
      'next_param',
      'source_params',
    ]

    sourceGroupFields.forEach((field) => {
      if (field in item && !(field in group)) {
        group[field] = item[field]
      }
      delete item[field]
    })

    const groupEntries = group.entries
    if (Array.isArray(groupEntries)) {
      groupEntries.push(item)
    }
  })

  return groups
}

/**
 * Builds next-page source params from pagination fields exposed by each search group.
 *
 * Pagination is intentionally source-scoped: each source may expose `total_pages`, `have_more`,
 * `next_value`, or explicit `source_params` through YAML/actions.
 *
 * @param groups Search groups returned by the backend.
 * @param fallbackPage Page requested by the frontend when a group does not expose `current_page`.
 * @returns Source params to send to the next search request.
 */
function buildSearchNextSourceParams(
  groups: JsonRecord[],
  fallbackPage: number,
): Record<string, string>[] {
  return dedupeSearchSourceParams(
    groups.flatMap((group) => buildNextParamsForSearchGroup(group, fallbackPage)),
  )
}

/**
 * Reads explicit next-source params emitted by YAML when a source needs custom pagination keys.
 *
 * @param group - The search group to read params from.
 * @param source - The source identifier for the group.
 * @returns Array of source param records for the next request.
 */
function readSearchExplicitSourceParams(
  group: JsonRecord,
  source: string,
): Record<string, string>[] {
  const rawSourceParams = readRecordList(group.source_params).map(readStringParamMap)

  if (rawSourceParams.length === 0) {
    rawSourceParams.push(...readRecordList(group.sourceParams).map(readStringParamMap))
  }

  return rawSourceParams
    .map((params) => ({ source, ...params }))
    .filter((params) => Object.keys(params).length > 1)
}

/**
 * Converts one source search group into the params expected by the next backend request.
 *
 * @param group - The search group to build params for.
 * @param fallbackPage - The page number to use when current_page is not available.
 * @returns Array of param records for the next request.
 */
function buildNextParamsForSearchGroup(
  group: JsonRecord,
  fallbackPage: number,
): Record<string, string>[] {
  const source = firstNonEmptyString([group.source, readPath(group, 'source', 'name')])
  if (!source) {
    return []
  }

  const explicitSourceParams = readSearchExplicitSourceParams(group, source)
  if (explicitSourceParams.length > 0) {
    return explicitSourceParams
  }

  const currentPage =
    toPositiveInteger(firstNumber(group.current_page)) ?? Math.max(1, Math.trunc(fallbackPage))
  const totalPages = toPositiveInteger(firstNumber(group.total_pages))
  const hasMoreFromTotal = totalPages !== null ? currentPage < totalPages : null
  const hasMore = readOptionalBoolean(group.have_more) ?? hasMoreFromTotal ?? false

  if (!hasMore) {
    return []
  }

  const nextParams: Record<string, string> = { source }
  const nextValue = firstNonEmptyString([group.next_value, group.nextValue])
  const nextParam = firstNonEmptyString([group.next_param, group.nextParam])

  if (nextValue) {
    nextParams[nextParam ?? 'page'] = nextValue
  } else {
    nextParams.page = String(currentPage + 1)
  }

  return [nextParams]
}

/**
 * Deduplicates search source params while preserving the first-seen order.
 *
 * @param paramsList - Array of param records to deduplicate.
 * @returns Deduplicated array of param records.
 */
function dedupeSearchSourceParams(paramsList: Record<string, string>[]): Record<string, string>[] {
  const seen = new Set<string>()
  const dedupedParams: Record<string, string>[] = []

  paramsList.forEach((params) => {
    const serializedParams = JSON.stringify(
      Object.entries(params).sort(([left], [right]) => left.localeCompare(right)),
    )

    if (seen.has(serializedParams)) {
      return
    }

    seen.add(serializedParams)
    dedupedParams.push({ ...params })
  })

  return dedupedParams
}
