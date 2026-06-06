import type { MediaItem } from '@/types/media'
import type {
  EntryDetails,
  EntryEpisode,
  EntryEpisodePage,
  EntryPlayer,
  EntryPlayerStoryboard,
  EntryPlayerResolver,
  EntryResolvedPlayerStream,
  EntrySeason,
} from '@/types/entry'
import type {
  HomeBanner,
  HomeCatalogData,
  HomeCategory,
  HomeCategorySource,
  HomeSection,
  HomeSectionSource,
} from '@/types/home'
import { t, tm } from '@/i18n'
import type { ServiceMetadata, ServiceThemeMetadata } from '@/types/serviceMetadata'

const restApiBaseUrl = import.meta.env.VITE_RUSTIFY_API_BASE_URL ?? '/api'

export const mediaTypeValues = [
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

export interface SearchFilterOption {
  value: string
  label: string
}

type JsonRecord = Record<string, unknown>

/**
 * Search filters sent to the backend catalog endpoint.
 */
export interface SearchMediaItemsFilters {
  mediaTypes?: string[]
  themes?: string[]
}

export interface SearchMediaItemsPage {
  items: MediaItem[]
  currentPage: number
  haveMore: boolean
  sourceParams: Record<string, string>[]
}

interface TauriWindow extends Window {
  __TAURI__?: {
    invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>
    core?: {
      invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>
    }
  }
}

/**
 * Calls a backend function through either Tauri or the REST bridge.
 *
 * @param fct_name Backend function name.
 * @param params Function parameters.
 * @returns Deserialized backend payload.
 */
export async function call_api<T = unknown>(
  fct_name: string,
  params: Record<string, unknown>,
): Promise<T> {
  const tauriWindow = window as TauriWindow
  const tauriInvoke =
    tauriWindow.__TAURI__?.core?.invoke ?? tauriWindow.__TAURI__?.invoke

  if (tauriInvoke) {
    return tauriInvoke<T>(fct_name, params)
  }

  const response = await fetch(`${restApiBaseUrl}/${fct_name}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(params),
  })

  const payload = await parseResponseBody(response)

  if (!response.ok) {
    throw new Error(extractErrorMessage(payload) ?? 
      t('errors.restCallFailed', { function: fct_name, status: response.status }))
  }

  return payload as T
}

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
  const items: MediaItem[] = []

  groups.forEach((group) => {
    const source = firstNonEmptyString([group.source, readPath(group, 'source', 'name')])

    readRecordList(group.entries).forEach((entry) => {
      items.push(normalizeMediaItem(entry, items.length, source))
    })
  })

  return {
    items,
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
 */
function readSearchExplicitSourceParams(
  group: JsonRecord,
  source: string,
): Record<string, string>[] {
  const rawSourceParams = readRecordList(group.source_params).map(readStringMap)

  if (rawSourceParams.length === 0) {
    rawSourceParams.push(...readRecordList(group.sourceParams).map(readStringMap))
  }

  return rawSourceParams
    .map((params) => ({ source, ...params }))
    .filter((params) => Object.keys(params).length > 1)
}

/**
 * Converts one source search group into the params expected by the next backend request.
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

/**
 * Loads the aggregated home payload exposed by the backend and normalizes it for the UI.
 *
 * @returns Featured banners, clickable categories, and grouped catalog rails.
 */
export async function loadHomeCatalog(): Promise<HomeCatalogData> {
  const response = await call_api<unknown>('load_home', {})

  return normalizeHomeCatalog(response)
}

/**
 * Loads display metadata for all configured backend services.
 *
 * @returns Normalized service metadata records indexed by their `id` field.
 */
export async function loadServiceMetadata(): Promise<ServiceMetadata[]> {
  const response = await call_api<unknown>('get_service', {})

  if (!Array.isArray(response)) {
    throw new Error(t('errors.unexpectedMetadataResponse'))
  }

  return response
    .map(normalizeServiceMetadata)
    .filter((metadata): metadata is ServiceMetadata => Boolean(metadata))
}

/**
 * Loads one aggregated category payload exposed by the backend and normalizes it for the UI.
 *
 * @param category Category descriptor previously returned by `loadHomeCatalog`.
 * @param page 1-based category page requested by the frontend.
 * @returns Normalized banners, categories, and grouped catalog rails for the selected category.
 */
export async function getCategoryCatalog(category: HomeCategory, page = 1): Promise<HomeCatalogData> {
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

  return normalizeHomeCatalog(response)
}

/**
 * Loads one additional page for a lazily loaded home section.
 *
 * @param section Section descriptor previously returned by `loadHomeCatalog`.
 * @param page 1-based visual page requested by the frontend.
 * @returns Normalized section payload returned by all section sources.
 */
export async function loadHomeSectionPage(section: HomeSection, page = 1): Promise<HomeSection> {
  if (section.sources.length === 0) {
    return section
  }

  const response = await call_api<unknown>('get_section', {
    page,
    sourceParams: section.sources.map((source) => ({
      source: source.name,
      link: source.link,
      ...source.params,
      page: String(page),
    })),
  })

  const responseRecord: JsonRecord = isJsonRecord(response) ? response : {}
  const responseSection = readRecordList(responseRecord.sections)[0] ?? responseRecord

  const normalizedSection = normalizeHomeSection(
    {
      label: section.label,
      entries: readRecordList(responseSection.entries),
      current_page: responseSection.current_page ?? page,
      have_more: responseSection.have_more ?? false,
    },
    0,
    null,
  )

  return {
    ...section,
    items: dedupeMediaItems([...section.items, ...normalizedSection.items]),
    currentPage: normalizedSection.currentPage,
    haveMore: normalizedSection.haveMore,
    sources: section.sources.map((source) => ({
      ...source,
      currentPage: normalizedSection.currentPage,
      haveMore: normalizedSection.haveMore,
    })),
  }
}

/**
 * Executes the backend entry endpoint and normalizes the response for the featured detail UI.
 *
 * @param source Backend source name that owns the entry.
 * @param entry Absolute entry URL sent to the Rust backend.
 * @param webUrl Public web URL opened by the featured action when available.
 * @returns Normalized detailed entry for the requested source.
 */
export async function getEntryDetails(
  source: string,
  entry: string,
  webUrl: string | null = null,
): Promise<EntryDetails> {
  const response = await call_api<unknown[]>('get_entry', { source, entry })

  if (!Array.isArray(response) || response.length === 0) {
    throw new Error(t('errors.unexpectedEntryResponseFormat'))
  }

  return normalizeEntryDetails(response[0], source, webUrl ?? entry)
}

/**
 * Executes the backend live listing endpoint and normalizes the response for the live details UI.
 *
 * @returns Normalized live media cards consumable by the shared collection components.
 */
export async function listLiveMediaItems(): Promise<MediaItem[]> {
  const response = await call_api<unknown[]>('list_lives', {})

  if (!Array.isArray(response)) {
    throw new Error(t('errors.unexpectedLiveListResponseFormat'))
  }

  return response.map((entry, index) => normalizeMediaItem(entry, index))
}

/**
 * Executes the backend live endpoint and normalizes the returned embedded players.
 *
 * @param source Backend source name that owns the live channel.
 * @param channel Source-specific live channel identifier.
 * @returns Normalized players available for the selected live channel.
 */
export async function getLivePlayers(source: string, channel: string): Promise<EntryPlayer[]> {
  const response = await call_api<unknown>('get_live', { source, channel })

  if (!isJsonRecord(response)) {
    throw new Error(t('errors.unexpectedLiveResponseFormat'))
  }

  return normalizeEntryPlayers(response, source)
}

/**
 * Converts one raw backend home/category payload into the frontend home catalog model.
 *
 * @param payload Raw backend home/category payload.
 * @returns Normalized home catalog data ready for rendering.
 */
function normalizeHomeCatalog(payload: unknown): HomeCatalogData {
  if (!Array.isArray(payload) && !isJsonRecord(payload)) {
    throw new Error(t('errors.unexpectedHomeResponseFormat'))
  }

  const rows = readRecordList(payload)
  const banners: HomeBanner[] = []
  const categories: HomeCategory[] = []
  const sections: HomeSection[] = []

  rows.forEach((row) => {
    const rowSource = firstNonEmptyString([row.source, readPath(row, 'source', 'name')])

    readBannerList(row.banners).forEach((banner) => {
      const normalizedBanner = normalizeHomeBanner(banner, banners.length, rowSource)
      if (
        normalizedBanner.entryUrl &&
        (normalizedBanner.imageUrl || normalizedBanner.title || normalizedBanner.videoUrl)
      ) {
        banners.push(normalizedBanner)
      }
    })

    readRecordList(row.categories).forEach((category) => {
      const normalizedCategory = normalizeHomeCategory(category, categories.length, rowSource)
      if (normalizedCategory.sources.length > 0) {
        categories.push(normalizedCategory)
      }
    })

    readRecordList(row.sections).forEach((section) => {
      const normalizedSection = normalizeHomeSection(section, sections.length, rowSource)
      if (normalizedSection.items.length > 0 || normalizedSection.sources.length > 0) {
        sections.push(normalizedSection)
      }
    })
  })

  return mergeNormalizedCatalogs([
    {
      banners,
      categories,
      sections,
    },
  ])
}

/**
 * Converts one backend service row into frontend display metadata.
 *
 * @param payload Raw backend row returned by `get_service`.
 * @returns Normalized service metadata, or `null` when the row does not expose an id.
 */
function normalizeServiceMetadata(payload: unknown): ServiceMetadata | null {
  const record = isJsonRecord(payload) ? payload : {}
  const id = firstNonEmptyString([record.id])

  if (!id) {
    return null
  }

  return {
    id,
    title: firstNonEmptyString([record.title]) ?? id,
    logo: firstNonEmptyString([record.logo]),
    description: normalizeServiceDescription(record.description),
    themes: normalizeServiceThemes(record.themes ?? record.search_themes),
  }
}

/**
 * Reads localized service descriptions from either a structured object or JSON string.
 *
 * @param value Backend description node.
 * @returns Language-keyed description strings.
 */
function normalizeServiceDescription(value: unknown): Record<string, string> {
  if (isJsonRecord(value) && !Array.isArray(value)) {
    return readStringMap(value)
  }

  const rawDescription = firstNonEmptyString([value])
  if (!rawDescription) {
    return {}
  }

  try {
    const parsedDescription: unknown = JSON.parse(rawDescription)
    return isJsonRecord(parsedDescription) ? readStringMap(parsedDescription) : {}
  } catch {
    return {}
  }
}

/**
 * Converts backend theme mapping rows into frontend service theme metadata.
 *
 * @param value Backend themes node.
 * @returns Theme mappings declared by the service.
 */
function normalizeServiceThemes(value: unknown): ServiceThemeMetadata[] {
  if (Array.isArray(value) && value.every((theme) => typeof theme === 'string')) {
    return value
      .map((theme) => theme.trim())
      .filter((theme) => theme.length > 0)
      .map((theme) => ({ code: theme, serviceCode: theme }))
  }

  return readRecordList(value)
    .map((theme) => {
      const code = firstNonEmptyString([theme.code])
      const serviceCode = firstNonEmptyString([theme.service_code, theme.serviceCode])

      if (!code || !serviceCode) {
        return null
      }

      return { code, serviceCode }
    })
    .filter((theme): theme is ServiceThemeMetadata => Boolean(theme))
}

/**
 * Merges multiple normalized catalogs into one display-ready payload.
 *
 * @param catalogs Normalized catalogs produced from one or more backend responses.
 * @returns Aggregated catalog with merged categories and merged section rails.
 */
function mergeNormalizedCatalogs(catalogs: HomeCatalogData[]): HomeCatalogData {
  const banners = catalogs.flatMap((catalog) => catalog.banners)
  const categories = mergeHomeCategories(catalogs.flatMap((catalog) => catalog.categories))
  const sections = mergeHomeSections(catalogs.flatMap((catalog) => catalog.sections))

  return {
    banners,
    categories,
    sections,
  }
}

/**
 * Converts one raw backend banner into the frontend featured banner model.
 *
 * @param payload Raw backend banner payload.
 * @param index Stable fallback index used to build a unique banner id.
 * @returns Normalized featured banner.
 */
function normalizeHomeBanner(
  payload: unknown,
  index: number,
  inheritedSource: string | null = null,
): HomeBanner {
  const record = isJsonRecord(payload) ? payload : {}
  const source = firstNonEmptyString([record.source, readPath(record, 'source', 'name'), inheritedSource])
  const entryUrl = resolveEntryUrl(
    firstNonEmptyString([record.entry_url, record.entryUrl, record.link]),
    source,
  )
  const webUrl = resolveEntryUrl(
    firstNonEmptyString([record.web_url, record.webUrl, record['web-link'], record.link]),
    source,
  )
  const imageUrl = resolveAssetUrl(
    firstNonEmptyString([record.image_url, record.imageUrl, record.image]),
    source,
  )
  const logoUrl = resolveAssetUrl(
    firstNonEmptyString([record.logo_url, record.logoUrl, record.logo]),
    source,
  )
  const videoUrl = resolveAssetUrl(
    firstNonEmptyString([record.video_url, record.videoUrl, record.video]),
    source,
  )
  const title = firstNonEmptyString([record.title])

  return {
    id:
      buildScopedId(firstNonEmptyString([record.id, record.key]), source) ??
      buildMediaId(index, entryUrl ?? imageUrl, title, source),
    title,
    subtitle: firstNonEmptyString([record.subtitle]),
    description: firstNonEmptyString([record.description]),
    imageUrl,
    logoUrl,
    videoUrl,
    source,
    entryUrl: entryUrl ?? webUrl,
    webUrl: webUrl ?? entryUrl,
  }
}

/**
 * Converts one raw backend category into the frontend clickable category model.
 *
 * @param payload Raw backend category payload.
 * @param index Stable fallback index used to build a unique category id.
 * @returns Normalized home category.
 */
function normalizeHomeCategory(
  payload: unknown,
  index: number,
  inheritedSource: string | null = null,
): HomeCategory {
  const record = isJsonRecord(payload) ? payload : {}
  const label = firstNonEmptyString([record.label]) ?? t('category.defaultLabel', { index: String(index + 1) })
  const source = normalizeHomeCategorySource(record, inheritedSource)
  const mergeKey = buildHomeCategoryMergeKey(firstNonEmptyString([record.key]), label, index)

  return {
    id:
      buildScopedId(
        firstNonEmptyString([record.id, record.key]) ?? mergeKey,
        inheritedSource,
      ) ??
      `category-${index + 1}`,
    label,
    imageUrl: firstNonEmptyString([record.image_url, record.imageUrl, record.image]),
    description: firstNonEmptyString([record.description]),
    mergeKey,
    sources: source ? [source] : [],
  }
}

/**
 * Merges categories that share the same semantic key across multiple sources.
 *
 * @param categories Normalized categories collected from one or more sources.
 * @returns Categories merged by semantic key while preserving first-seen order.
 */
function mergeHomeCategories(categories: HomeCategory[]): HomeCategory[] {
  const categoriesByMergeKey = new Map<string, HomeCategory>()
  const mergedCategories: HomeCategory[] = []

  categories.forEach((category) => {
    const existingCategory = categoriesByMergeKey.get(category.mergeKey)
    if (!existingCategory) {
      const mergedCategory: HomeCategory = {
        ...category,
        sources: dedupeHomeCategorySources(category.sources),
      }

      categoriesByMergeKey.set(category.mergeKey, mergedCategory)
      mergedCategories.push(mergedCategory)
      return
    }

    existingCategory.label ||= category.label
    existingCategory.imageUrl ??= category.imageUrl
    existingCategory.description ??= category.description
    existingCategory.sources = dedupeHomeCategorySources([
      ...existingCategory.sources,
      ...category.sources,
    ])
  })

  return mergedCategories
}

/**
 * Converts one raw backend category source into a frontend source descriptor.
 *
 * @param record Raw backend category payload.
 * @param inheritedSource Source inherited from the parent scraper row.
 * @returns Normalized category source, or `null` when the payload is unusable.
 */
function normalizeHomeCategorySource(
  record: JsonRecord,
  inheritedSource: string | null,
): HomeCategorySource | null {
  const source = readStringMap(record.source)

  if (Object.keys(source).length > 0) {
    return source
  }

  if (!inheritedSource) {
    return null
  }

  return {
    name: inheritedSource,
    ...readStringMap(record.request),
  }
}

/**
 * Builds the semantic merge key used to collapse categories across sources.
 *
 * @param rawKey Backend category key when provided by the source config.
 * @param label Display label used as a fallback merge input.
 * @param index Stable fallback index used to guarantee a non-empty key.
 * @returns Category merge key shared by equivalent categories.
 */
function buildHomeCategoryMergeKey(rawKey: string | null, label: string, index: number): string {
  const normalizedKey = normalizeString(rawKey ?? '')?.toLocaleLowerCase()
  if (normalizedKey) {
    return normalizedKey
  }

  return slugify(label) || `category-${index + 1}`
}

/**
 * Converts one raw backend section into the frontend grouped collection model.
 *
 * @param payload Raw backend section payload.
 * @param index Stable fallback index used to build a unique section id.
 * @returns Normalized home section.
 */
function normalizeHomeSection(
  payload: unknown,
  index: number,
  inheritedSource: string | null = null,
): HomeSection {
  const record = isJsonRecord(payload) ? payload : {}
  let rawItems = readRecordList(record.items)
  if (rawItems.length === 0) {
    rawItems = readRecordList(record.entries)
  }
  const label = firstNonEmptyString([record.label])
  const currentPage = Math.max(1, Math.trunc(firstNumber(record.current_page) ?? 1))
  const haveMore = readBoolean(record.have_more)
  const sources = normalizeHomeSectionSources(record, inheritedSource, currentPage, haveMore)
  const id =
    buildScopedId(
      firstNonEmptyString([record.id, record.key]) ?? (slugify(label ?? '') || `section-${index + 1}`),
      inheritedSource,
    ) ??
    `section-${index + 1}`

  return {
    id,
    label,
    preferenceKey: buildHomeSectionPreferenceKey(label, id),
    items: rawItems.map((item, itemIndex) => normalizeMediaItem(item, itemIndex, inheritedSource)),
    sources,
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
 * @returns Section source descriptors usable by `get_section`.
 */
function normalizeHomeSectionSources(
  record: JsonRecord,
  inheritedSource: string | null,
  currentPage: number,
  haveMore: boolean,
): HomeSectionSource[] {
  const link = firstNonEmptyString([record.link, record.query_url, record.queryUrl])

  if (!link || !inheritedSource) {
    return []
  }

  return [
    {
      name: inheritedSource,
      link,
      currentPage,
      haveMore,
      params: readStringMap(record.request),
    },
  ]
}

/**
 * Merges section rails that share the same visible label.
 *
 * @param sections Normalized section rails collected from one or more sources.
 * @returns Aggregated section rails with deduplicated media items.
 */
function mergeHomeSections(sections: HomeSection[]): HomeSection[] {
  const sectionsByPreferenceKey = new Map<string, HomeSection>()
  const mergedSections: HomeSection[] = []

  sections.forEach((section) => {
    const existingSection = sectionsByPreferenceKey.get(section.preferenceKey)

    if (!existingSection) {
      const mergedSection: HomeSection = {
        ...section,
        items: dedupeMediaItems(section.items),
        sources: dedupeHomeSectionSources(section.sources),
      }

      sectionsByPreferenceKey.set(section.preferenceKey, mergedSection)
      mergedSections.push(mergedSection)
      return
    }

    existingSection.label ??= section.label
    existingSection.items = dedupeMediaItems([...existingSection.items, ...section.items])
    existingSection.sources = dedupeHomeSectionSources([
      ...existingSection.sources,
      ...section.sources,
    ])
    existingSection.currentPage = Math.max(existingSection.currentPage, section.currentPage)
    existingSection.haveMore = existingSection.haveMore || section.haveMore
  })

  return mergedSections
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
    const serializedSource = JSON.stringify([
      source.name,
      source.link,
      Object.entries(source.params).sort(([left], [right]) => left.localeCompare(right)),
    ])

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
 * Executes the backend season endpoint and normalizes the response for the entry detail UI.
 *
 * @param source Backend source name that owns the season.
 * @param season Absolute season URL sent to the Rust backend.
 * @param page 1-based page number requested from the backend.
 * @param seasonName Season label used when the backend payload does not expose one per episode.
 * @returns Normalized paged season payload for the requested season.
 */
export async function getSeasonEpisodes(
  source: string,
  season: string,
  page = 1,
  seasonName: string | null = null,
): Promise<EntryEpisodePage> {
  const response = await call_api<unknown>('get_season', { source, season, page })

  return normalizeSeasonEpisodePage(response, source, seasonName, page)
}

/**
 * Resolves one backend player into a directly playable stream when the source needs a custom
 * playback handshake such as DRM token retrieval.
 *
 * @param source Backend source name that owns the selected player.
 * @param player Normalized player descriptor selected in the UI.
 * @returns Resolved stream payload, or `null` when the player does not need extra resolution.
 */
export async function resolvePlayerStream(
  source: string,
  player: EntryPlayer,
): Promise<EntryResolvedPlayerStream | null> {
  if (!player.resolver) {
    return null
  }

  const response = await call_api<unknown>('resolve_player_stream', {
    source,
    resolverKind: player.resolver.kind,
    resolverTarget: player.resolver.targetId,
    resolverStreamKind: player.resolver.streamKind,
  })

  return normalizeResolvedPlayerStream(response)
}

/**
 * Parses a REST response body while tolerating empty payloads.
 *
 * @param response Fetch response returned by the REST bridge.
 * @returns Parsed JSON payload or `null` when the body is empty.
 */
async function parseResponseBody(response: Response): Promise<unknown> {
  const body = await response.text()

  if (!body) {
    return null
  }

  try {
    return JSON.parse(body)
  } catch {
    return body
  }
}

/**
 * Extracts a displayable message from an unknown backend error payload.
 *
 * @param payload Backend error payload.
 * @returns Human-readable error text when available.
 */
function extractErrorMessage(payload: unknown): string | null {
  if (typeof payload === 'string') {
    return normalizeString(payload)
  }

  if (!isJsonRecord(payload)) {
    return null
  }

  const directMessage = firstNonEmptyString([
    payload.message,
    payload.error,
    payload.details,
  ])

  if (directMessage) {
    return directMessage
  }

  return null
}

/**
 * Converts one raw backend entry into the frontend media card model.
 *
 * @param entry Raw backend search entry.
 * @param index Stable fallback index used to build a unique card id.
 * @returns Normalized media item.
 */
function normalizeMediaItem(
  entry: unknown,
  index: number,
  inheritedSource: string | null = null,
): MediaItem {
  const record = isJsonRecord(entry) ? entry : {}
  const source = firstNonEmptyString([record.source, readPath(record, 'source', 'name'), inheritedSource])
  const link = firstNonEmptyString([record.link])
  const webLink = firstNonEmptyString([record['web-link'], record.webLink, record.link])
  const rawTitle = firstNonEmptyString([record.title])
  const altTitle = firstNonEmptyString([record['title/alt']])
  const title = firstNonEmptyString([rawTitle, altTitle, record.label])
  const alternativeTitleLabel =
    rawTitle && altTitle && rawTitle.trim() !== altTitle.trim() ? altTitle : null
  const imagePosterUrl = resolveAssetUrl(
    readFirstLink(record, 'img/poster', ['img', 'poster']),
    source,
  )
  const imagePortraitUrl = resolveAssetUrl(
    readFirstLink(record, 'img/portrait', ['img', 'portrait']),
    source,
  )
  const imageLandscapeUrl = resolveAssetUrl(
    readFirstLink(record, 'img/landscape', ['img', 'landscape']),
    source,
  )
  const entryUrl = resolveEntryUrl(link, source)
  const webUrl = resolveEntryUrl(webLink, source)
  const mediaTypeValues = dedupeDisplayStrings(readStringList(record['media-type']))
  const mediaTypeLabel = firstNonEmptyString(readStringList(record['media-type']).map(toDisplayMediaType))
  const themeLabels = dedupeDisplayStrings(readStringList(record.theme))
  const audioLabel = firstNonEmptyString([record.lang, record.audio, record.language])
  const durationLabel = formatDurationLabel(firstNonEmptyString([record.duration]))
  const rating = firstNumber(record.rating)
  const overview = firstNonEmptyString([record.description, record.overview])
  const episodeLabel = firstNonEmptyString([readPath(record, 'episode', 'label')])
  const releaseDateLabel = formatReleaseDateLabel(firstNonEmptyString([record['release-date']]))
  const expireLabel = formatReleaseDateLabel(firstNonEmptyString([record.expire]))
  const metaLine = buildMetaLine(record)

return {
     id: buildMediaId(index, link, title, source),
     title,
     alternativeTitleLabel,
     imagePosterUrl,
     imagePortraitUrl,
     imageLandscapeUrl,
     source,
    entryUrl,
    webUrl,
    mediaTypeLabel,
    mediaTypeValues,
    themeLabels,
    audioLabel,
    durationLabel,
    rating,
    overview,
    episodeLabel,
    releaseDateLabel,
    expireLabel,
    metaLine,
  }
}

/**
 * Converts one raw backend entry into the frontend detail model.
 *
 * @param entry Raw backend entry returned by `get_entry`.
 * @param source Source name used for the request.
 * @param entryUrl Absolute entry URL used for the request.
 * @returns Normalized detailed media entry.
 */
function normalizeEntryDetails(entry: unknown, source: string, entryUrl: string): EntryDetails {
  const record = isJsonRecord(entry) ? entry : {}
  const seasons = normalizeEntrySeasons(record, source)
  const episodes = normalizeEntryEpisodes(record, source)
  const players = normalizeEntryPlayers(record, source)
  const imagePosterUrl = resolveAssetUrl(readFirstLink(record, 'img/poster', ['img', 'poster']), source)
  const imagePortraitUrl = resolveAssetUrl(readFirstLink(record, 'img/portrait', ['img', 'portrait']), source)
  const imageLandscapeUrl = resolveAssetUrl(readFirstLink(record, 'img/landscape', ['img', 'landscape']), source)
  const trailerUrl = firstNonEmptyString([record['video/trailer']])
  const webLink = resolveEntryUrl(
    firstNonEmptyString([record['web-link'], record.webLink, record.web_url, record.webUrl]),
    source,
  )

   return {
      source,
      entryUrl: webLink ?? entryUrl,
      title: firstNonEmptyString([record.title]),
      alternativeTitleLabel: firstNonEmptyString([record['title/alt']]),
      trailerUrl,
      players,
      description: firstNonEmptyString([record.description]),
      imagePosterUrl,
      imagePortraitUrl,
      imageLandscapeUrl,
      logoUrl: resolveAssetUrl(readFirstLink(record, 'img/logo', ['img', 'logo']), source),
      heroImageUrl: imagePortraitUrl ?? imagePosterUrl,
     yearLabel: firstNonEmptyString([record.year]),
     releaseDateLabel: formatReleaseDateLabel(firstNonEmptyString([record['release-date']])),
     expireLabel: formatReleaseDateLabel(firstNonEmptyString([record.expire])),
     durationLabel: formatDurationLabel(firstNonEmptyString([record.duration])),
     seasonCountLabel: formatSeasonCount(firstNonEmptyString([record['count-season']])),
     audioLanguageLabel: buildAudioLanguageLabel(record),
     subtitleLanguageLabel: buildSubtitleLanguageLabel(record),
     contentAdvisorLabel: firstNonEmptyString([record['content-advisor']]),
     themeLabels: dedupeDisplayStrings(readStringList(record.theme)),
     genreLabels: dedupeDisplayStrings(readStringList(record.genre)),
     castingLabels: dedupeDisplayStrings(readStringList(record.casting)),
     directorLabels: dedupeDisplayStrings(readStringList(record.director)),
     seasons,
     episodes,
   }
 }

/**
 * Converts raw backend player records into normalized embedded player entries.
 *
 * @param players Raw backend player entries.
 * @param source Source name used to resolve embed URLs.
 * @returns Normalized embedded players.
 */
function normalizePlayers(
  players: JsonRecord[],
  source: string,
): EntryPlayer[] {
  return players
    .map((player, index) => normalizeEntryPlayer(player, index, source))
    .filter((player): player is EntryPlayer => Boolean(player))
}

/**
 * Converts backend player lists into normalized embedded player entries.
 *
 * @param entry Raw backend entry returned by `get_entry`.
 * @param source Source name used to resolve embed URLs.
 * @returns Normalized embedded players.
 */
function normalizeEntryPlayers(entry: JsonRecord, source: string): EntryPlayer[] {
  return normalizePlayers(readRecordList(entry.players), source)
}

/**
 * Converts one raw backend player entry into the frontend detail model.
 *
 * @param entry Raw backend player entry.
 * @param index Stable fallback index used to build a unique player id.
 * @param source Source name used to resolve embed URLs.
 * @returns Normalized player, or `null` when no usable embed URL exists.
 */
function normalizeEntryPlayer(
  entry: JsonRecord,
  index: number,
  source: string,
): EntryPlayer | null {
  const embedLink = resolveEntryUrl(firstNonEmptyString([entry['embed-link']]), source)
  const directLink = resolveEntryUrl(
    firstNonEmptyString([entry['direct-link'], entry.directLink]),
    source,
  )
  const resolver = normalizeEntryPlayerResolver(entry)
  const name = firstNonEmptyString([entry.name])
  const lang = firstNonEmptyString([entry.lang])
  const label = buildPlayerLabel(name, lang, index)
  const storyboard = normalizeEntryPlayerStoryboard(entry, source)

  if (!embedLink && !directLink && !resolver) {
    return null
  }

  if (!embedLink && !directLink) {
    return {
      id: buildMediaId(index, resolver?.targetId ?? null, label, source),
      label,
      embedLink: null,
      directLink: null,
      name,
      lang,
      resolver,
      storyboard,
    }
  }

  return {
    id: buildMediaId(index, directLink ?? embedLink, label, source),
    label,
    embedLink,
    directLink,
    name,
    lang,
    resolver,
    storyboard,
  }
}

/**
 * Converts one backend player resolver payload into the frontend player resolver model.
 *
 * @param entry Raw backend player entry.
 * @returns Normalized resolver descriptor, or `null` when the player is self-contained.
 */
function normalizeEntryPlayerResolver(entry: JsonRecord): EntryPlayerResolver | null {
  const resolver = readRecordList(entry.resolver)[0] ?? null
  const resolverStream = resolver ? readRecordList(readPath(resolver, 'stream'))[0] ?? null : null

  const kind = firstNonEmptyString([
    readPath(resolver, 'kind'),
    readPath(entry, 'resolverKind'),
  ])
  const targetId = firstNonEmptyString([
    readPath(resolver, 'target_id'),
    readPath(resolver, 'targetId'),
    readPath(entry, 'resolverTarget'),
  ])

  if (!kind || !targetId) {
    return null
  }

  const streamKind = firstNonEmptyString([
    readPath(resolverStream, 'kind'),
    readPath(resolver, 'streamKind'),
    readPath(entry, 'resolverStreamKind'),
  ])

  return {
    kind,
    targetId,
    streamKind,
  }
}

/**
 * Converts backend storyboard metadata into a Video.js sprite thumbnail configuration.
 *
 * @param entry Raw backend player entry.
 * @param source Source name used to resolve storyboard assets.
 * @returns Normalized storyboard configuration, or `null` when incomplete.
 */
function normalizeEntryPlayerStoryboard(
  entry: JsonRecord,
  source: string,
): EntryPlayerStoryboard | null {
  const url = resolveAssetUrl(
    firstNonEmptyString([
      readPath(entry, 'storyboard', 'link'),
      readPath(entry, 'storyboard', 'url'),
    ]),
    source,
  )
  const width = toPositiveInteger(firstNumber(readPath(entry, 'storyboard', 'width')))
  const height = toPositiveInteger(firstNumber(readPath(entry, 'storyboard', 'height')))
  const columns = toPositiveInteger(firstNumber(readPath(entry, 'storyboard', 'columns')))
  const configuredInterval = firstNumber(readPath(entry, 'storyboard', 'interval'))

  if (!url || !width || !height || !columns) {
    return null
  }

  const interval =
    configuredInterval !== null && Number.isFinite(configuredInterval) && configuredInterval > 0
      ? configuredInterval
      : null

  if (interval === null || !Number.isFinite(interval) || interval <= 0) {
    return null
  }

  return {
    url,
    width,
    height,
    columns,
    interval,
  }
}

/**
 * Normalizes one backend player resolution payload into the frontend stream model.
 *
 * @param payload Raw backend payload returned by `resolve_player_stream`.
 * @returns Normalized stream payload, or `null` when the backend returned nothing usable.
 */
function normalizeResolvedPlayerStream(payload: unknown): EntryResolvedPlayerStream | null {
  if (!isJsonRecord(payload)) {
    return null
  }

  const streamUrl = firstNonEmptyString([payload.stream_url, payload.streamUrl])
  const manifestType = firstNonEmptyString([payload.manifest_type, payload.manifestType])

  if (!streamUrl || !manifestType) {
    return null
  }

  return {
    streamUrl,
    manifestType,
    licenseUrl: firstNonEmptyString([payload.license_url, payload.licenseUrl]),
    licenseHeaders: readStringMap(payload.license_headers ?? payload.licenseHeaders),
  }
}

/**
 * Converts the backend season lists into normalized season entries.
 *
 * @param entry Raw backend entry returned by `get_entry`.
 * @param source Source name used to resolve the season links.
 * @returns Normalized season entries.
 */
function normalizeEntrySeasons(entry: JsonRecord, source: string): EntrySeason[] {
  return readRecordList(entry.season)
    .map((season, index) => {
      const label = firstNonEmptyString([season.label])
      const normalizedLabel = label ? normalizeString(label) : null

      if (!normalizedLabel) {
        return null
      }

      const link = resolveEntryUrl(firstNonEmptyString([season.link]), source)

      return {
        id: buildMediaId(index, link, normalizedLabel, source),
        label: normalizedLabel,
        link,
      }
    })
    .filter((season): season is EntrySeason => Boolean(season))
}

/**
 * Converts eager episode lists returned directly by `get_entry`.
 *
 * @param entry Raw backend entry returned by `get_entry`.
 * @param source Source name used to resolve episode links and media.
 * @returns Normalized episodes embedded in the entry payload.
 */
function normalizeEntryEpisodes(entry: JsonRecord, source: string): EntryEpisode[] {
  return readRecordList(entry.episode)
    .map((episode, index) => normalizeEntryEpisode(episode, index, source))
    .filter((episode) => !isUnavailableEpisode(episode))
}

/**
 * Converts one raw backend episode into the frontend episode detail model.
 *
 * @param entry Raw backend episode entry.
 * @param index Stable fallback index used to build a unique episode id.
 * @param source Source name used to resolve media assets.
 * @param fallbackSeasonName Season label used when the backend response omits it.
 * @returns Normalized episode detail.
 */
function normalizeEntryEpisode(
  entry: JsonRecord,
  index: number,
  source: string,
  fallbackSeasonName: string | null = null,
): EntryEpisode {
  const previewEntries = readRecordList(readPath(entry, 'img/preview'))
  const imagePosterUrl = firstNonEmptyString([readPath(entry, 'img/poster'), readPath(entry, 'img/preview')])
  const link = resolveEntryUrl(firstNonEmptyString([entry.link]), source)
  const duration = firstNonEmptyString([entry.duration])
  const players = normalizePlayers(readRecordList(entry.players), source)
  const rawTitle = firstNonEmptyString([entry.title])
  const altTitle = firstNonEmptyString([entry['title/alt']])
  const title = rawTitle || altTitle
  const alternativeTitleLabel =
    rawTitle && altTitle && rawTitle.trim() !== altTitle.trim() ? altTitle : null

  return {
    id: buildMediaId(index, link, title, source),
    link,
    players,
    seasonName: firstNonEmptyString([entry['season-name'], fallbackSeasonName]),
    title,
    alternativeTitleLabel,
    description: firstNonEmptyString([entry.description]),
    releaseDateLabel: formatReleaseDateLabel(firstNonEmptyString([entry['release-date']])),
    expireLabel: formatReleaseDateLabel(firstNonEmptyString([entry.expire])),
    durationLabel: formatDurationLabel(duration),
    previewUrl: resolveAssetUrl(
      firstNonEmptyString([imagePosterUrl, ...previewEntries.map((preview) => preview.link)]),
      source,
    ),
  }
}

/**
 * Builds a display-ready player label from the backend player metadata.
 *
 * @param name Player host or backend-provided display name.
 * @param lang Optional player language label.
 * @param index Stable fallback index used when no metadata is available.
 * @returns Display-ready player label.
 */
function buildPlayerLabel(name: string | null, lang: string | null, index: number): string {
  if (name) {
    return name
  }

  return t('player.defaultLabel', { index: String(index + 1) })
}

/**
 * Converts one paged backend season response into normalized episode data and pagination metadata.
 *
 * @param payload Raw backend payload returned by `get_season`.
 * @param source Source name used to resolve episode media.
 * @param seasonName Season label used when the backend payload does not expose one per episode.
 * @param fallbackPage Requested page number used when the backend omits page metadata.
 * @returns Normalized paged season response.
 */
function normalizeSeasonEpisodePage(
  payload: unknown,
  source: string,
  seasonName: string | null,
  fallbackPage: number,
): EntryEpisodePage {
  if (!isJsonRecord(payload)) {
    throw new Error(t('errors.unexpectedSeasonResponseFormat'))
  }

  const episodes = Array.isArray(payload.episodes) ? payload.episodes : []

  return {
    currentPage: Math.max(1, Math.trunc(firstNumber(payload.current_page) ?? fallbackPage)),
    haveMore: readBoolean(payload.have_more),
    episodes: episodes
      .map((entry, index) =>
        normalizeEntryEpisode(isJsonRecord(entry) ? entry : {}, index, source, seasonName),
      )
      .filter((episode) => !isUnavailableEpisode(episode)),
  }
}

/**
 * Builds a display title for one episode.
 *
 * @param entry Raw backend episode entry.
 * @returns Normalized episode title.
 */
function buildEpisodeTitle(entry: JsonRecord): string | null {
  return firstNonEmptyString([entry.title, entry['title/alt']])
}

/**
 * Returns whether one normalized episode should be hidden from the UI.
 *
 * @param episode Normalized episode entry.
 * @returns `true` when the episode title is empty or marked unavailable.
 */
function isUnavailableEpisode(episode: EntryEpisode): boolean {
  const title = episode.title?.trim().toLocaleLowerCase()
  return !title || title === 'titre indisponible'
}

/**
 * Builds a compact metadata line from the backend fields available on a result.
 *
 * @param entry Raw backend entry.
 * @returns Short metadata line displayed in the preview panel.
 */
function buildMetaLine(entry: JsonRecord): string | null {
  const topicValues = [
    ...readStringList(entry.genre),
    ...readStringList(entry.theme),
  ]
  const mediaTypeValues = readStringList(entry['media-type']).flatMap((value) => {
    const label = toDisplayMediaType(value)
    return label ? [label] : []
  })
  const languageValues = readStringList(entry.lang)
  const values = dedupeDisplayStrings([...topicValues, ...mediaTypeValues, ...languageValues])

  return values.length > 0 ? values.join(' • ') : null
}

/**
 * Reads a nested property from an unknown JSON-like object.
 *
 * @param value Root value to inspect.
 * @param path Nested property path.
 * @returns Nested value or `null` when the path does not exist.
 */
function readPath(value: unknown, ...path: string[]): unknown {
  let current: unknown = value

  for (const segment of path) {
    if (!isJsonRecord(current)) {
      return null
    }

    current = current[segment]
  }

  return current
}

/**
 * Collects flattened strings from serialized scraper nodes.
 *
 * @param value Unknown backend node.
 * @returns Flat list of trimmed non-empty strings.
 */
function readStringList(value: unknown): string[] {
  if (typeof value === 'string') {
    const normalized = normalizeString(value)
    return normalized ? [normalized] : []
  }

  if (Array.isArray(value)) {
    return value.flatMap((item) => readStringList(item))
  }

  if (!isJsonRecord(value)) {
    return []
  }

  if ('_' in value) {
    return readStringList(value._)
  }

  return []
}

/**
 * Reads a string-only record from an unknown backend object shape.
 *
 * @param value Unknown backend object that may contain scraper scalar nodes.
 * @returns Plain string record containing the first non-empty value for each key.
 */
function readStringMap(value: unknown): Record<string, string> {
  if (!isJsonRecord(value)) {
    return {}
  }

  const output: Record<string, string> = {}

  Object.entries(value).forEach(([key, entryValue]) => {
    const normalizedValue = firstNonEmptyString([entryValue])

    if (normalizedValue) {
      output[key] = normalizedValue
    }
  })

  return output
}

/**
 * Collects plain-object records from a serialized scraper node.
 *
 * @param value Unknown backend node.
 * @returns Flat list of JSON-like records.
 */
function readRecordList(value: unknown): JsonRecord[] {
  if (Array.isArray(value)) {
    return value.filter(isJsonRecord)
  }

  return isJsonRecord(value) ? [value] : []
}

/**
 * Reads home banners while tolerating both arrays of banner objects and one indexed object payload.
 *
 * Some scraper outputs can serialize repeated banner fields into one object whose properties are
 * aligned arrays. When that happens, the frontend must rebuild one record per index instead of
 * treating the whole object as a single banner.
 *
 * @param value Unknown backend banner node.
 * @returns Banner records ready for normalization.
 */
function readBannerList(value: unknown): JsonRecord[] {
  if (Array.isArray(value)) {
    return value.flatMap((entry) => {
      if (isJsonRecord(entry)) {
        return expandIndexedBannerRecords([entry])
      }

      if (Array.isArray(entry)) {
        const fragments = entry.filter(isJsonRecord)
        return fragments.length > 0 ? [mergeBannerFragments(fragments)] : []
      }

      return []
    })
  }

  return expandIndexedBannerRecords(readRecordList(value))
}

/**
 * Expands one banner payload when the backend aligned several banner fields in one indexed object.
 *
 * @param records Candidate banner records extracted from the backend node.
 * @returns One or more banner records ready for normalization.
 */
function expandIndexedBannerRecords(records: JsonRecord[]): JsonRecord[] {
  if (records.length !== 1) {
    return records
  }

  const candidate = records[0]
  if (!candidate) {
    return []
  }

  const entries = Object.entries(candidate)
  const maxFieldLength = entries.reduce(
    (maxLength, [, fieldValue]) => Math.max(maxLength, readIndexedFieldValues(fieldValue).length),
    0,
  )

  if (maxFieldLength <= 1) {
    return records
  }

  return Array.from({ length: maxFieldLength }, (_, index) => {
    const record: JsonRecord = {}

    entries.forEach(([key, fieldValue]) => {
      const indexedValue = readIndexedFieldValues(fieldValue)[index]
      if (indexedValue !== undefined && indexedValue !== null) {
        record[key] = indexedValue
      }
    })

    return record
  }).filter((record) => Object.keys(record).length > 0)
}

/**
 * Rebuilds one banner record from several partial fragments emitted for the same backend item.
 *
 * Some Rust-serialized banner items are emitted as an array of partial objects when one field
 * contains more values than the others. The frontend only needs one consolidated record.
 *
 * @param fragments Partial records that all belong to the same banner item.
 * @returns Consolidated banner record.
 */
function mergeBannerFragments(fragments: JsonRecord[]): JsonRecord {
  const record: JsonRecord = {}

  fragments.forEach((fragment) => {
    Object.entries(fragment).forEach(([key, value]) => {
      if (!(key in record) && value !== null && value !== undefined) {
        record[key] = value
      }
    })
  })

  return record
}

/**
 * Returns the aligned values attached to one potentially indexed backend field.
 *
 * @param value Unknown backend field value.
 * @returns Raw values that can be consumed by one indexed banner record.
 */
function readIndexedFieldValues(value: unknown): unknown[] {
  if (Array.isArray(value)) {
    return value
  }

  if (isJsonRecord(value) && Array.isArray(value._)) {
    return value._
  }

  return value === null || value === undefined ? [] : [value]
}

/**
 * Reads the first available string value from a list of backend nodes.
 *
 * @param values Candidate backend nodes.
 * @returns First non-empty string value.
 */
function firstNonEmptyString(values: unknown[]): string | null {
  for (const value of values) {
    const candidate = readStringList(value)[0]

    if (candidate) {
      return candidate
    }
  }

  return null
}

/**
 * Parses the first numeric value available in a backend node.
 *
 * @param value Backend node that may contain a number-like string.
 * @returns Parsed number when available.
 */
function firstNumber(value: unknown): number | null {
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : null
  }

  const firstValue = firstNonEmptyString([value])

  if (!firstValue) {
    return null
  }

  const parsedValue = Number.parseFloat(firstValue)
  return Number.isFinite(parsedValue) ? parsedValue : null
}

/**
 * Normalizes one numeric backend value into a positive integer.
 *
 * @param value Parsed numeric backend value.
 * @returns Positive integer when available.
 */
function toPositiveInteger(value: number | null): number | null {
  if (value === null) {
    return null
  }

  const normalizedValue = Math.trunc(value)
  return Number.isFinite(normalizedValue) && normalizedValue > 0 ? normalizedValue : null
}

/**
 * Reads a boolean value while tolerating raw booleans and stringified booleans.
 *
 * @param value Backend node that may contain a boolean-like value.
 * @returns Parsed boolean when available, otherwise `false`.
 */
function readBoolean(value: unknown): boolean {
  if (typeof value === 'boolean') {
    return value
  }

  const firstValue = firstNonEmptyString([value])?.toLocaleLowerCase()

  if (!firstValue) {
    return false
  }

  return firstValue === 'true'
}

/**
 * Reads a boolean only when the backend explicitly emitted a boolean-like value.
 *
 * @param value Backend node that may contain a boolean-like value.
 * @returns Parsed boolean, or `null` when the field is absent/unknown.
 */
function readOptionalBoolean(value: unknown): boolean | null {
  if (typeof value === 'boolean') {
    return value
  }

  const firstValue = firstNonEmptyString([value])?.toLocaleLowerCase()
  if (!firstValue) {
    return null
  }

  if (firstValue === 'true') {
    return true
  }

  if (firstValue === 'false') {
    return false
  }

  return null
}

/**
 * Reads a media link from either the flat scraper key shape or a nested object shape.
 *
 * @param record Raw backend record.
 * @param flatKey Flat field name such as `img/poster`.
 * @param nestedPath Fallback nested path when the payload is object-shaped.
 * @returns First resolved raw link string.
 */
function readFirstLink(record: JsonRecord, flatKey: string, nestedPath: string[]): string | null {
  return firstNonEmptyString([
    readPath(record, flatKey, 'link'),
    readPath(record, ...nestedPath, 'link'),
  ])
}

/**
 * Builds the displayable audio language label from the backend language-related fields.
 *
 * @param record Raw backend record.
 * @returns Combined audio language label when available.
 */
function buildAudioLanguageLabel(record: JsonRecord): string | null {
  const values = dedupeDisplayStrings([
    ...readStringList(record.language),
    ...readStringList(readPath(record, 'lang/audio')),
  ])

  return values.length > 0 ? values.join(' • ') : null
}

/**
 * Builds the displayable subtitle language label from the backend language-related fields.
 *
 * @param record Raw backend record.
 * @returns Combined subtitle language label when available.
 */
function buildSubtitleLanguageLabel(record: JsonRecord): string | null {
  const values = dedupeDisplayStrings(readStringList(readPath(record, 'lang/subtitles')))

  return values.length > 0 ? values.join(' • ') : null
}

/**
 * Resolves a media asset URL against the source base URL when the backend returns a relative path.
 *
 * @param value Raw asset URL returned by the backend.
 * @param source Source name associated with the asset.
 * @returns Absolute asset URL when it can be resolved.
 */
function resolveAssetUrl(value: string | null, source: string | null): string | null {
  if (!value) {
    return null
  }

  if (/^https?:\/\//i.test(value)) {
    return value
  }

  if (value.startsWith('//')) {
    return `https:${value}`
  }

   return value
}

/**
 * Resolves an entry URL against the source base URL when the backend returns a relative path.
 *
 * @param value Raw entry link returned by the backend search query.
 * @param source Source name associated with the entry.
 * @returns Absolute entry URL when it can be resolved.
 */
function resolveEntryUrl(value: string | null, source: string | null): string | null {
  if (!value) {
    return null
  }

  const normalizedValue = normalizeString(value)
  if (!normalizedValue) {
    return null
  }

  if (/^https?:\/\//i.test(normalizedValue)) {
    return normalizedValue
  }

  if (normalizedValue.startsWith('//')) {
    return `https:${normalizedValue}`
  }

  return normalizedValue
}


/**
 * Formats a season count extracted from the backend entry.
 *
 * @param value Raw season count.
 * @returns Display-ready season count label.
 */
function formatSeasonCount(value: string | null): string | null {
  if (!value) {
    return null
  }

  const parsedValue = Number.parseInt(value, 10)

  if (!Number.isFinite(parsedValue)) {
    return value
  }

  return parsedValue > 1
    ? t('entry.seasonCountPlural', { count: String(parsedValue) })
    : t('entry.seasonCountSingular', { count: String(parsedValue) })
}

/**
 * Formats a backend duration expressed in seconds into `h:mm:ss` or `mm:ss`.
 *
 * @param value Raw duration string returned by the backend.
 * @returns Display-ready duration label.
 */
function formatDurationLabel(value: string | null): string | null {
  if (!value) {
    return null
  }

  const totalSeconds = parseDurationSeconds(value)

  if (totalSeconds === null) {
    return value
  }

  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const seconds = totalSeconds % 60
  const minutesLabel = String(minutes).padStart(2, '0')
  const secondsLabel = String(seconds).padStart(2, '0')

  if (hours > 0) {
    return `${hours}:${minutesLabel}:${secondsLabel}`
  }

  return `${minutesLabel}:${secondsLabel}`
}

/**
 * Parses a backend duration expressed in seconds.
 *
 * @param value Raw duration string returned by the backend.
 * @returns Parsed duration in seconds when valid.
 */
function parseDurationSeconds(value: string | null): number | null {
  if (!value || !/^\d+$/.test(value)) {
    return null
  }

  const totalSeconds = Number.parseInt(value, 10)
  return Number.isFinite(totalSeconds) && totalSeconds >= 0 ? totalSeconds : null
}

/**
 * Formats a backend release date expressed as `YYYY-MM-DD` into `DD/MM/YYYY`.
 *
 * @param value Raw release date returned by the backend.
 * @returns Display-ready release date label.
 */
function formatReleaseDateLabel(value: string | null): string | null {
  if (!value) {
    return null
  }

  const match = value.match(/^(\d{4})-(\d{2})-(\d{2})$/)
  if (match) {
      const [, year, month, day] = match
      return `${day}/${month}/${year}`
  }

  const match2 = value.match(/^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})$/)
  if (match2) {
      const [, year, month, day, hh, mm] = match2
      return `${day}/${month}/${year} ${hh}:${mm}`
  }

  return value;
}

/**
 * Produces a stable frontend id from the backend result contents.
 *
 * @param index Search result index.
 * @param link Canonical entry URL when available.
 * @param title Display title used as a fallback.
 * @param source Backend source associated with the item.
 * @returns Stable card identifier.
 */
function buildMediaId(
  index: number,
  link: string | null,
  title: string | null,
  source: string | null = null,
): string {
  const baseValue = [normalizeString(source ?? ''), link, title, `media-${index + 1}`]
    .filter((value): value is string => Boolean(value))
    .join('-')
  const normalizedBaseValue = slugify(baseValue) || `media-${index + 1}`

  return `${normalizedBaseValue}-${index + 1}`
}

/**
 * Deduplicates category source descriptors while preserving their first-seen order.
 *
 * @param sources Category source descriptors attached to one merged category.
 * @returns Deduplicated source descriptors.
 */
function dedupeHomeCategorySources(sources: HomeCategorySource[]): HomeCategorySource[] {
  const seen = new Set<string>()
  const dedupedSources: HomeCategorySource[] = []

  sources.forEach((source) => {
    const serializedSource = serializeHomeCategorySource(source)
    if (seen.has(serializedSource)) {
      return
    }

    seen.add(serializedSource)
    dedupedSources.push({ ...source })
  })

  return dedupedSources
}

/**
 * Serializes one category source descriptor into a stable comparison key.
 *
 * @param source Category source descriptor.
 * @returns Stable string representation used for deduplication.
 */
function serializeHomeCategorySource(source: HomeCategorySource): string {
  return JSON.stringify(
    Object.entries(source).sort(([left], [right]) => left.localeCompare(right)),
  )
}

/**
 * Deduplicates media items while preserving their first-seen order.
 *
 * @param items Media items collected inside one merged section rail.
 * @returns Deduplicated media items.
 */
function dedupeMediaItems(items: MediaItem[]): MediaItem[] {
  const seen = new Set<string>()
  const dedupedItems: MediaItem[] = []

  items.forEach((item) => {
    const dedupeKey = buildMediaDeduplicationKey(item)
    if (seen.has(dedupeKey)) {
      return
    }

    seen.add(dedupeKey)
    dedupedItems.push(item)
  })

  return dedupedItems
}

/**
 * Builds the comparison key used to collapse duplicate media items in merged rails.
 *
 * @param item Media item rendered inside a catalog rail.
 * @returns Stable deduplication key.
 */
function buildMediaDeduplicationKey(item: MediaItem): string {
  const normalizedSource = normalizeString(item.source ?? '')
  const normalizedEntryUrl = normalizeString(item.entryUrl ?? '')
  if (normalizedSource && normalizedEntryUrl) {
    return `entry:${normalizedSource}:${normalizedEntryUrl}`
  }

  const normalizedWebUrl = normalizeString(item.webUrl ?? '')
  if (normalizedSource && normalizedWebUrl) {
    return `web:${normalizedSource}:${normalizedWebUrl}`
  }

  const normalizedTitle = normalizeString(item.title ?? '')
  const normalizedPosterUrl = normalizeString(item.imagePosterUrl ?? '')
  if (normalizedSource && normalizedTitle && normalizedPosterUrl) {
    return `poster:${normalizedSource}:${normalizedTitle.toLocaleLowerCase()}:${normalizedPosterUrl}`
  }

  return `id:${item.id}`
}

/**
 * Prefixes a backend identifier with its source to avoid cross-source collisions.
 *
 * @param id Raw identifier returned by the backend.
 * @param source Backend source associated with the item.
 * @returns Scoped identifier or `null` when no id is available.
 */
function buildScopedId(id: string | null, source: string | null): string | null {
  const normalizedId = normalizeString(id ?? '')
  if (!normalizedId) {
    return null
  }

  const normalizedSource = normalizeString(source ?? '')
  return normalizedSource ? `${normalizedSource}:${normalizedId}` : normalizedId
}

/**
 * Normalizes a display string by trimming surrounding whitespace.
 *
 * @param value Raw string value.
 * @returns Trimmed string or `null` when empty.
 */
function normalizeString(value: string): string | null {
  const normalized = value.trim()
  return normalized.length > 0 ? normalized : null
}

/**
 * Converts backend media-type identifiers into user-facing labels.
 *
 * @param value Raw media type returned by the backend.
 * @returns Display-ready media type label.
 */
function toDisplayMediaType(value: string): string | undefined {
  if (value.startsWith("video/other/")) {
    return value.split("video/other/")[1];
  } if (value.startsWith("video/show/other/")) {
    return value.split("video/show/other/")[1];
  } else {
    return tm('mediaTypes')[value] ?? value
  }
}

/**
 * Deduplicates display strings while preserving their original casing.
 *
 * @param values Candidate display strings.
 * @returns Deduplicated list that preserves the first display value.
 */
function dedupeDisplayStrings(values: string[]): string[] {
  const seen = new Set<string>()
  const returnValue: string[] = []

  values.forEach((value) => {
    const normalizedValue = normalizeString(value)

    if (!normalizedValue) {
      return false
    }

    const key = normalizedValue.toLocaleLowerCase()

    if (seen.has(key)) {
      return false
    }

    seen.add(key)
    returnValue.push(normalizedValue)
    return true
  })

  return returnValue
}

/**
 * Creates a URL-safe identifier fragment from a backend string.
 *
 * @param value Raw identifier source.
 * @returns Slugified identifier fragment.
 */
function slugify(value: string): string {
  return value
    .toLocaleLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
}

/**
 * Checks whether an unknown value is a plain object record.
 *
 * @param value Value to inspect.
 * @returns `true` when the value can be accessed as a key/value record.
 */
function isJsonRecord(value: unknown): value is JsonRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}
