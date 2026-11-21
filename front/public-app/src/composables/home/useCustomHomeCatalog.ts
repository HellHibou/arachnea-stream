import { computed, onScopeDispose, shallowReactive, shallowRef, toValue, watch, type MaybeRefOrGetter } from 'vue'
import { createCategoryCatalogResolver, resolveCatalogSection } from '@/services/customHomeCategoryResolver'
import { bannerReferenceKey, categoryOrigin } from '@/services/customHomeReferences'
import { mediaRouteIdentity } from '@/services/mediaRouting'
import { sessionGeneration } from '@/services/sessionEvents'
import { validateCustomHomeDefinition } from '@/services/customHomeValidation'
import { getCategoryCatalog, loadHomeSectionPage, loadHomeSourceDiscovery, dedupeMediaItems, interleaveLists } from '@/services/rustify'
import { appendCategorySections, hasMoreCategoryPages } from '@/services/customHomeCategorySections'
import { composeCustomHomeSections, type CustomHomeContribution } from '@/services/customHomeComposition'
import { mergeHomeCategories } from '@/services/rustify/home/categories'
import { loadDeferredBannerPages } from '@/composables/home/deferredBannerLoader'
import type { BannerReference, CustomHomeBlock, CustomHomeDefinition, HomeSourceDiscovery } from '@/types/customHome'
import type { HomeBanner, HomeCatalogData, HomeCategory, HomeSection } from '@/types/home'

/** Recoverable discovery, reference, hydration or pagination issue. */
export interface CustomHomeResolutionIssue {
  id: string
  message: string
  repairRequired: boolean
}

/** @returns Detached empty catalog; no discovery or hydration is implied. */
function emptyCatalog(): HomeCatalogData {
  return { banners: { entries: [], source: '' }, deferredBanners: [], categories: [], sections: [] }
}

/**
 * Shares only in-flight immutable responses, never mutable block pagination state.
 * @param key Stable request identity including descriptor and page.
 * @param loader Request factory.
 * @returns Pending response shared across repeated blocks and mounted catalogs.
 */
function requestOnce<T>(key: string, loader: () => Promise<T>): Promise<T> {
  const scopedKey = `${sessionGeneration()}:${key}`
  const existing = pending.get(scopedKey)
  if (existing) return existing as Promise<T>
  const request = loader().finally(() => { pending.delete(scopedKey) })
  pending.set(scopedKey, request)
  return request
}
const pending = new Map<string, Promise<unknown>>()

/** @param category Exact current source descriptor. @param page Requested category page. @returns Unmerged catalog. */
function categoryPage(category: HomeCategory, page: number): Promise<HomeCatalogData> {
  const key = JSON.stringify(category.sources.map((source) => Object.entries(source).sort(([a], [b]) => a.localeCompare(b))))
  return requestOnce(`category:${key}:${page}`, () => getCategoryCatalog(category, page, false, true))
}

/**
 * Resolves selected content, preserving block ownership, partial failures and request revisions.
 * @param input Reactive persisted definition or detached editor draft; null performs no network calls.
 * @returns Catalog loader interface and contribution-owned pagination/retry actions.
 */
export function useCustomHomeCatalog(input: MaybeRefOrGetter<CustomHomeDefinition | null>) {
  const catalog = shallowRef<HomeCatalogData | null>(null)
  const isLoading = shallowRef(false)
  const hasLoaded = shallowRef(false)
  const errorMessage = shallowRef<string | null>(null)
  const latestRequestId = shallowRef(0)
  const issues = shallowRef<CustomHomeResolutionIssue[]>([])
  const loadingIds = shallowRef(new Set<string>())
  const members = shallowRef<CustomHomeContribution[]>([])
  let definition: CustomHomeDefinition | null = null
  let base = emptyCatalog()
  let sourceCatalogs = new Map<string, HomeSourceDiscovery>()
  const categories = shallowReactive(new Map<string, { category: HomeCategory; page: number; haveMore: boolean }>())
  let categoryResolver = createCategoryCatalogResolver()
  const bannerCatalogs = new Map<string, HomeCatalogData>()
  let activeWorkers = 0
  const waiters: Array<() => void> = []

  /** @param task Selected hydration task. @returns Result with at most four concurrent section requests. */
  async function limited<T>(task: () => Promise<T>): Promise<T> {
    if (activeWorkers >= 4) await new Promise<void>((resolve) => waiters.push(resolve))
    else activeWorkers++
    try { return await task() }
    finally {
      const next = waiters.shift()
      if (next) next()
      else activeWorkers--
    }
  }

  /** @param id Selection/member identity. @param error Contextual failure. @param repairRequired Missing identity requires editing. */
  function report(id: string, error: unknown, repairRequired = false): void {
    issues.value = [...issues.value.filter((issue) => issue.id !== id), {
      id, message: error instanceof Error ? error.message : String(error), repairRequired,
    }]
  }

  /** Rebuilds render groups while preserving current banners and shortcuts. */
  function publish(): void {
    if (definition) catalog.value = { ...base, sections: composeCustomHomeSections(definition, members.value,
      new Map([...categories].map(([id, state]) => [id, state.category.label]))) }
  }

  /** @param block Category content or a section owned by a category. @param revision Active catalog request. @param refresh Rediscover a missing reference. */
  async function loadCategoryBlock(block: CustomHomeBlock, revision: number, refresh = false): Promise<void> {
    const section = block.kind === 'home-section' ? block.reference : undefined
    const reference = block.kind === 'category-content' ? block.reference : block.reference.category
    if (!reference) return
    const id = `category:${block.id}`
    if (loadingIds.value.has(id)) return
    const previous = categories.get(block.id)
    if (previous && !previous.haveMore) return
    loadingIds.value = new Set([...loadingIds.value, id])
    let category = previous?.category
    try {
      const resolver = categoryResolver
      if (refresh && !previous) {
        sourceCatalogs.delete(reference.sourceId)
        resolver.clear()
      }
      category ??= await resolver.resolve(categoryOrigin(reference))
      if (revision !== latestRequestId.value) return
      if (!previous) categories.set(block.id, { category, page: 0, haveMore: true })
      const descriptor = category
      const page = (previous?.page ?? 0) + 1
      const result = await limited(() => {
        if (revision !== latestRequestId.value) return Promise.resolve(emptyCatalog())
        return page === 1 ? resolver.load(reference).then((entry) => entry.catalog) : categoryPage(descriptor, page)
      })
      if (revision !== latestRequestId.value) return
      const selected = section ? resolveCatalogSection(result, section) : undefined
      if (section && page === 1 && !selected) {
        report(id, new Error(`Category section missing or ambiguous: ${section.sectionKey}`), true)
        return
      }
      const catalogPage = section ? { ...result, sections: selected ? [selected] : [] } : result
      const previousSections = members.value.filter((member) => member.blockId === block.id)
      const sections = appendCategorySections(block.id, previousSections, catalogPage.sections, page)
      const order = definition?.blocks ?? []
      const index = order.findIndex((item) => item.id === block.id)
      const before = new Set(order.slice(0, index).map((item) => item.id))
      const retained = members.value.filter((member) => member.blockId !== block.id)
      const insertAt = retained.findIndex((member) => !before.has(member.blockId))
      retained.splice(insertAt < 0 ? retained.length : insertAt, 0, ...sections)
      members.value = retained
      const linked = (rail: HomeSection) => rail.sources.length || rail.subsections?.some((variant) => variant.sources.length)
      const haveMore = hasMoreCategoryPages(catalogPage) &&
        (!section || !sections.length || !sections.every((member) => linked(member.section)))
      categories.set(block.id, { category, page, haveMore })
      issues.value = issues.value.filter((issue) => issue.id !== id)
    } catch (error) {
      if (revision === latestRequestId.value) report(id, error,
        !category && Boolean(sourceCatalogs.get(reference.sourceId)?.available))
    } finally {
      if (revision === latestRequestId.value) {
        loadingIds.value = new Set([...loadingIds.value].filter((key) => key !== id))
        publish()
      }
    }
  }

  const categoryBlocks = computed(() => {
    const sectionCounts = new Map<string, number>()
    for (const section of catalog.value?.sections ?? []) {
      for (const id of section.customBlockIds ?? []) {
        sectionCounts.set(id, (sectionCounts.get(id) ?? 0) + 1)
      }
    }
    return (toValue(input)?.blocks ?? []).flatMap((block) => {
      if (!catalog.value || (block.kind === 'home-section' && !block.reference.category)) return []
      const state = categories.get(block.id)
      const id = `category:${block.id}`
      const count = sectionCounts.get(block.id) ?? 0
      return [{
        id: block.id,
        title: block.titleOverride?.trim() ||
          (block.kind === 'category-content' ? state?.category.label : block.reference.labelSnapshot) ||
          block.reference.labelSnapshot || state?.category.label || '',
        showTitle: !count,
        heading: count > 1 ? block.titleOverride?.trim() || null : null,
        category: state?.category,
        empty: Boolean(block.kind === 'category-content' && state && !state.haveMore && !count),
        haveMore: Boolean(state?.haveMore),
        loading: loadingIds.value.has(id),
        issue: issues.value.find((issue) => issue.id === id),
      }]
    })
  })

  /** @param blockId Category block whose failed or next page should be loaded. */
  async function loadCategoryPage(blockId: string): Promise<void> {
    const block = definition?.blocks.find((item) => item.id === blockId)
    if (!block) return
    const repair = issues.value.some((issue) => issue.id === `category:${blockId}` && issue.repairRequired)
    if (repair) categories.delete(blockId)
    await loadCategoryBlock(block, latestRequestId.value, repair || !categories.has(blockId))
  }

  /** Loads only required discoveries and category blocks; stale responses cannot publish. */
  async function loadCatalog(): Promise<void> {
    const revision = ++latestRequestId.value
    catalog.value = null
    isLoading.value = true
    hasLoaded.value = false
    errorMessage.value = null
    issues.value = []
    loadingIds.value = new Set()
    members.value = []
    sourceCatalogs = new Map()
    categories.clear()
    base = emptyCatalog()
    bannerCatalogs.clear()
    try {
      const value = toValue(input)
      definition = value === null ? null : validateCustomHomeDefinition(value)
      if (!definition) return
      const current = definition
      categoryResolver = createCategoryCatalogResolver(
        async (id) => {
          const known = sourceCatalogs.get(id)
          if (known?.available) return known
          const result = (await requestOnce(`discovery:${id}`, () => loadHomeSourceDiscovery([id])))[0]
          if (result && revision === latestRequestId.value) sourceCatalogs.set(id, result)
          return result
        },
        (category) => categoryPage(category, 1),
      )
      const ids = [...new Set([...current.bannerSources.map((ref) => typeof ref === 'string' ? ref : ref.sourceId),
        ...current.categoryShortcuts.map((reference) => reference.sourceId),
        ...current.blocks.map((block) => block.reference.sourceId)])]
      const discoveries = await Promise.all(ids.map(async (sourceId) => {
        try {
          const rows = await requestOnce(`discovery:${sourceId}`, () => loadHomeSourceDiscovery([sourceId]))
          return rows[0]
        } catch (error) {
          if (revision === latestRequestId.value) report(`source:${sourceId}`, error)
          return undefined
        }
      }))
      if (revision !== latestRequestId.value) return
      sourceCatalogs = new Map(discoveries.filter((row): row is HomeSourceDiscovery => Boolean(row)).map((row) => [row.sourceId, row]))
      const shortcutLoads = Promise.all(current.categoryShortcuts.map(async (reference, index) => {
        try {
          const origin = categoryOrigin(reference)
          const category = await categoryResolver.resolve(origin)
          return { ...category, customShortcutIndex: index, origins: [origin] }
        } catch (error) {
          if (revision === latestRequestId.value) {
            report(`shortcut:${index}`, error, Boolean(sourceCatalogs.get(reference.sourceId)?.available))
          }
          return undefined
        }
      })).then((shortcuts) => {
        if (revision !== latestRequestId.value) return
        const categories = mergeHomeCategories(shortcuts.flatMap((category, index) => {
          if (!category) return []
          const title = current.categoryShortcuts[index]?.titleOverride?.trim()
          return title ? [{ ...category, label: title }] : [category]
        }))
        base = { ...base, categories }
        publish()
      })
      members.value = current.blocks.flatMap((block): CustomHomeContribution[] => {
        if (block.kind === 'category-content' || block.reference.category) return []
        try {
          const source = sourceCatalogs.get(block.reference.sourceId)
          if (!source?.available) throw new Error(`Source unavailable: ${block.reference.sourceId}`)
          const section = resolveCatalogSection(source.catalog, block.reference)
          if (!section) throw new Error(`Section missing or ambiguous: ${block.reference.sectionKey}`)
          return [{ id: JSON.stringify([block.id, block.reference.sectionKey, block.reference.subsectionKey ?? null]),
            blockId: block.id, section }]
        } catch (error) {
          report(`block:${block.id}`, error, Boolean(sourceCatalogs.get(block.reference.sourceId)?.available))
          return []
        }
      })
      const categoryLoads = current.blocks.filter((block) => block.kind === 'category-content' || block.reference.category)
        .map((block) => loadCategoryBlock(block, revision))
      const bannerLoads = Promise.all(current.bannerSources.map((reference) => loadBannerReference(reference, revision)))
        .then(() => { if (revision === latestRequestId.value) publishBanners() })
      publish()
      await Promise.all([shortcutLoads, bannerLoads, ...categoryLoads])
    } catch (error) {
      if (revision === latestRequestId.value) errorMessage.value = error instanceof Error ? error.message : String(error)
    } finally {
      if (revision === latestRequestId.value) { isLoading.value = false; hasLoaded.value = true }
    }
  }

  /** @param lists Banners in configured source order. @returns Interleaved, source/content-deduplicated entries. */
  function interleaveBanners(lists: HomeBanner[][]): HomeBanner[] {
    const seen = new Set<string>()
    return interleaveLists(lists).filter((banner) => {
      const key = JSON.stringify([banner.source, banner.entryUrl ?? banner.id])
      if (seen.has(key)) return false
      seen.add(key)
      return true
    })
  }

  /** Publishes banner collections in definition order, each with its own deferred links. */
  function publishBanners(): void {
    const catalogs = (definition?.bannerSources ?? []).flatMap((reference) => {
      const resolved = bannerCatalogs.get(bannerReferenceKey(reference))
      return resolved ? [resolved] : []
    })
    base = {
      ...base,
      banners: { ...base.banners, entries: interleaveBanners(catalogs.map((item) => item.banners.entries)) },
      deferredBanners: catalogs.flatMap((item) => item.deferredBanners),
    }
    publish()
  }

  /** @param reference Saved collection. @param revision Active catalog request. */
  async function loadBannerReference(reference: BannerReference, revision: number): Promise<void> {
    const key = bannerReferenceKey(reference)
    const id = `banners:${key}`
    if (loadingIds.value.has(id)) return
    loadingIds.value = new Set([...loadingIds.value, id])
    try {
      const resolver = categoryResolver
      const result = typeof reference === 'string' ? await resolver.source(reference) :
        (await limited(() => resolver.load(reference))).catalog
      if (revision !== latestRequestId.value) return
      bannerCatalogs.set(key, { ...result, deferredBanners: result.deferredBanners.filter((entry) => entry.link) })
      issues.value = issues.value.filter((issue) => issue.id !== id)
    } catch (error) {
      if (revision === latestRequestId.value) report(id, error)
    } finally {
      if (revision === latestRequestId.value) loadingIds.value = new Set([...loadingIds.value].filter((entry) => entry !== id))
    }
  }

  /** @param onlyKey Collection identity for a local retry, or all collections by default. */
  async function loadDeferredBanners(onlyKey?: string): Promise<void> {
    const revision = latestRequestId.value
    await Promise.all([...bannerCatalogs].map(async ([key, current]) => {
      const id = `banners:${key}`
      if ((onlyKey !== undefined && onlyKey !== key) || !current.deferredBanners.length || loadingIds.value.has(id)) return
      loadingIds.value = new Set([...loadingIds.value, id])
      const failed: HomeCatalogData['deferredBanners'] = []
      const loaded = await Promise.all(current.deferredBanners.map((collection) => limited(async () => {
        let succeeded = false
        const result = await loadDeferredBannerPages({ ...emptyCatalog(), deferredBanners: [collection] }, () => { succeeded = true }, true)
        return { collection, result, succeeded }
      })))
      if (revision !== latestRequestId.value) return
      const entries = [...current.banners.entries]
      for (const entry of loaded) {
        if (entry.succeeded) entries.push(...entry.result.banners.entries)
        else failed.push(entry.collection)
      }
      bannerCatalogs.set(key, { ...current, banners: { ...current.banners, entries }, deferredBanners: failed })
      if (failed.length) report(id, new Error(`Banner load failed: ${failed[0]!.source}`))
      else issues.value = issues.value.filter((issue) => issue.id !== id)
      loadingIds.value = new Set([...loadingIds.value].filter((entry) => entry !== id))
      publishBanners()
    }))
  }

  /** @param section Render group. @returns Its independently owned members, including subsection overlays. */
  function selectedMembers(section: HomeSection): CustomHomeContribution[] {
    return members.value.filter((member) => section.customContributionIds?.includes(member.id))
  }

  /** @param memberId Section contribution. @param subsectionKey Optional active variant. @returns Request and error key. */
  function sectionRequestId(memberId: string, subsectionKey?: string): string {
    return JSON.stringify([memberId, subsectionKey ?? null])
  }

  /** @param section Render group. @param initial Whether to fetch only missing first pages. */
  async function loadSection(section: HomeSection, initial = false): Promise<void> {
    const revision = latestRequestId.value
    const candidates = selectedMembers(section)
    const subsectionKey = section.customSubsectionKey
    const failedIds = new Set(issues.value.map((issue) => issue.id))
    const failed = candidates.filter((member) => failedIds.has(sectionRequestId(member.id, subsectionKey)))
    const selected = !initial && failed.length ? failed : candidates
    await Promise.all(selected.flatMap((member) => {
      const subsections = member.section.subsections ?? []
      if (initial && !subsectionKey && subsections.length) {
        return subsections.map((variant) => loadSectionTarget(member, variant.key, true, revision))
      }
      return [loadSectionTarget(member, subsectionKey, initial, revision)]
    }))
  }

  /**
   * Loads linked-section pages, preserving category cards received while the link was loading.
   * @param member Section contribution. @param subsectionKey Optional variant key.
   * @param initial Whether to hydrate only new links. @param revision Active catalog request.
   */
  async function loadSectionTarget(
    member: CustomHomeContribution, subsectionKey: string | undefined, initial: boolean, revision: number,
  ): Promise<void> {
    const id = sectionRequestId(member.id, subsectionKey)
    const variant = member.section.subsections?.find((item) => item.key === subsectionKey)
    const target = variant ? { ...member.section, ...variant } : member.section
    const sources = target.sources.filter((source) => !source.hasInitialItems || (!initial && source.haveMore))
    if (!sources.length || loadingIds.value.has(id)) return
    if (initial && issues.value.some((issue) => issue.id === id)) return
    loadingIds.value = new Set([...loadingIds.value, id])

    try {
      for (const source of sources) {
        if (revision !== latestRequestId.value) return
        const cursor = source.hasInitialItems ? source.currentPage + 1 : 1
        const key = JSON.stringify([source.name, source.link, Object.entries(source.params).sort(([a], [b]) => a.localeCompare(b)), cursor])
        const response = await limited(() => revision === latestRequestId.value
          ? requestOnce(`section:${key}`, () => loadHomeSectionPage({ ...target, items: [], sources: [source] }, [source], true))
          : Promise.resolve(target))
        if (revision !== latestRequestId.value) return

        const items = categories.has(member.blockId) ? response.items.filter(mediaRouteIdentity) : response.items
        members.value = members.value.map((item) => {
          if (item.id !== member.id) return item
          const current = item.section
          const currentVariant = current.subsections?.find((entry) => entry.key === subsectionKey)
          const rail = currentVariant ?? current
          const updatedSources = rail.sources.map((entry) => entry === source ? response.sources[0]! : entry)
          const update = {
            items: dedupeMediaItems([...rail.items, ...items]),
            sources: updatedSources,
            haveMore: updatedSources.some((entry) => entry.haveMore),
            currentPage: Math.max(rail.currentPage, response.currentPage),
          }
          return { ...item, section: currentVariant ? { ...current,
            subsections: current.subsections?.map((entry) => entry === currentVariant ? { ...entry, ...update } : entry),
          } : { ...current, ...update } }
        })
      }
      issues.value = issues.value.filter((issue) => issue.id !== id)
    } catch (error) {
      if (revision === latestRequestId.value) report(id, error)
    } finally {
      if (revision === latestRequestId.value) {
        loadingIds.value = new Set([...loadingIds.value].filter((key) => key !== id))
        publish()
      }
    }
  }

  /** @param section Render group. @returns Whether any active contribution is loading. */
  function isSectionLoading(section: HomeSection): boolean {
    return selectedMembers(section).some((member) =>
      loadingIds.value.has(sectionRequestId(member.id, section.customSubsectionKey)))
  }

  /** @param section Render group. @returns Errors for its active contributions. */
  function getSectionLoadError(section: HomeSection): string | null {
    const ids = section.customContributionIds?.map((id) => sectionRequestId(id, section.customSubsectionKey))
    return issues.value.filter((issue) => ids?.includes(issue.id)).map((issue) => issue.message).join('\n') || null
  }

  /** @param id Failed selection/member. Retries hydration locally, or rediscovers selected references. */
  async function retryIssue(id: string): Promise<void> {
    const reference = definition?.bannerSources.find((entry) => `banners:${bannerReferenceKey(entry)}` === id)
    if (reference) {
      const key = bannerReferenceKey(reference)
      if (bannerCatalogs.get(key)?.deferredBanners.length) await loadDeferredBanners(key)
      else {
        sourceCatalogs.delete(typeof reference === 'string' ? reference : reference.sourceId)
        categoryResolver.clear()
        const revision = latestRequestId.value
        await loadBannerReference(reference, revision)
        if (revision !== latestRequestId.value) return
        publishBanners()
        await loadDeferredBanners(key)
      }
      return
    }
    await loadCatalog()
  }
  watch(() => JSON.stringify(toValue(input)), () => { void loadCatalog() }, { immediate: true })
  onScopeDispose(() => { latestRequestId.value++ })
  return { catalog, isLoading, hasLoaded, errorMessage, latestRequestId, issues: computed(() => issues.value.filter((issue) => !issue.id.startsWith('category:') && !issue.id.startsWith('['))),
    categoryBlocks, loadCategoryPage,
    loadCatalog, loadDeferredBanners, loadSection, isSectionLoading, getSectionLoadError, retryIssue }
}
