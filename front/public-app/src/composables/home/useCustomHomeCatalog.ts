import { computed, onScopeDispose, shallowRef, toValue, watch, type MaybeRefOrGetter } from 'vue'
import { validateCustomHomeDefinition } from '@/services/customHomeValidation'
import { getCategoryCatalog, loadHomeSectionPage, loadHomeSourceDiscovery, dedupeMediaItems, interleaveLists } from '@/services/rustify'
import { composeCustomHomeSections, type CustomHomeContribution } from '@/services/customHomeComposition'
import { loadDeferredBannerPages } from '@/composables/home/deferredBannerLoader'
import type { CategoryReference, CustomHomeDefinition, HomeSourceDiscovery } from '@/types/customHome'
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
  const existing = pending.get(key)
  if (existing) return existing as Promise<T>
  const request = loader().finally(() => { pending.delete(key) })
  pending.set(key, request)
  return request
}
const pending = new Map<string, Promise<unknown>>()

/** @param category Exact current source descriptor. @param page Requested category page. @returns Unmerged catalog. */
function categoryPage(category: HomeCategory, page: number): Promise<HomeCatalogData> {
  const key = JSON.stringify(category.sources.map((source) => Object.entries(source).sort(([a], [b]) => a.localeCompare(b))))
  return requestOnce(`category:${key}:${page}`, () => getCategoryCatalog(category, page, false))
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
  let categories = new Map<string, { category: HomeCategory; page: number; haveMore: boolean }>()
  let bannersLoading = false
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
    if (definition) catalog.value = { ...base, sections: composeCustomHomeSections(definition, members.value) }
  }

  /** @param reference Saved category identity. @returns Exact rediscovered category, never a same-title replacement. */
  function resolveCategory(reference: CategoryReference): HomeCategory {
    const source = sourceCatalogs.get(reference.sourceId)
    if (!source?.available) throw new Error(`Source unavailable: ${reference.sourceId}`)
    const matches = source.catalog.categories.filter((category) =>
      category.identity?.key === reference.categoryKey && !category.identity.ambiguous)
    if (matches.length !== 1) throw new Error(`Category missing or ambiguous: ${reference.categoryKey}`)
    return matches[0]!
  }

  /** @param blockId Owning block. @param sections Unmerged columns. @param title Optional display override. @returns Detached members. */
  function columns(blockId: string, sections: HomeSection[], title: string | null): CustomHomeContribution[] {
    const seen = new Set<string>()
    return sections.map((section) => {
      const key = section.identity?.key ?? section.id
      if (section.identity?.ambiguous || seen.has(key)) throw new Error(`Category column identity is ambiguous: ${key}`)
      seen.add(key)
      return { id: JSON.stringify([blockId, key]), blockId, categoryPage: true,
        section: { ...section, label: title?.trim() || section.label } }
    })
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
    categories = new Map()
    base = emptyCatalog()
    bannersLoading = false
    try {
      const value = toValue(input)
      definition = value === null ? null : validateCustomHomeDefinition(value)
      if (!definition) return
      const current = definition
      const ids = [...new Set([...current.bannerSources,
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
      const bannerCatalogs = current.bannerSources.map((id) => {
        const row = sourceCatalogs.get(id)
        if (!row?.available) report(`banners:${id}`, new Error(`Source unavailable: ${id}`))
        return row?.catalog ?? emptyCatalog()
      })
      base.banners.entries = interleaveBanners(bannerCatalogs.map((item) => item.banners.entries))
      base.deferredBanners = bannerCatalogs.flatMap((item) => item.deferredBanners)
      base.categories = current.categoryShortcuts.flatMap((reference, index) => {
        try { return [{ ...resolveCategory(reference), customShortcutIndex: index }] }
        catch (error) { report(`shortcut:${index}`, error, Boolean(sourceCatalogs.get(reference.sourceId)?.available)); return [] }
      })
      const resolved = await Promise.all(current.blocks.map(async (block): Promise<CustomHomeContribution[]> => {
        try {
          if (block.kind === 'category-content') {
            const category = resolveCategory(block.reference)
            const result = await limited(() => revision === latestRequestId.value ? categoryPage(category, 1) : Promise.resolve(emptyCatalog()))
            if (revision !== latestRequestId.value) return []
            categories.set(block.id, { category, page: 1, haveMore: Boolean(result.haveMore) || result.sections.some((section) => section.haveMore && !section.sources.length) })
            return columns(block.id, result.sections, block.titleOverride).map((member) => ({ ...member,
              section: { ...member.section, haveMore: member.section.haveMore || Boolean(result.haveMore) } }))
          }
          const source = sourceCatalogs.get(block.reference.sourceId)
          if (!source?.available) throw new Error(`Source unavailable: ${block.reference.sourceId}`)
          const matches = source.catalog.sections.filter((section) => section.identity?.key === block.reference.sectionKey && !section.identity.ambiguous)
          if (matches.length !== 1) throw new Error(`Section missing or ambiguous: ${block.reference.sectionKey}`)
          let section = matches[0]!
          if (block.reference.subsectionKey !== undefined) {
            const variants = (section.subsections ?? []).filter((variant) => variant.identity?.key === block.reference.subsectionKey && !variant.identity?.ambiguous)
            if (variants.length !== 1) throw new Error(`Subsection missing or ambiguous: ${block.reference.subsectionKey}`)
            const variant = variants[0]!
            section = { ...section, ...variant, subsections: undefined }
          }
          return [{ id: JSON.stringify([block.id, block.reference.sectionKey, block.reference.subsectionKey ?? null]),
            blockId: block.id, categoryPage: false, section: { ...section, label: block.titleOverride?.trim() || section.label } }]
        } catch (error) {
          if (revision === latestRequestId.value) report(`block:${block.id}`, error,
            Boolean(sourceCatalogs.get(block.reference.sourceId)?.available) && block.kind === 'home-section')
          return []
        }
      }))
      if (revision !== latestRequestId.value) return
      members.value = resolved.flat()
      publish()
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

  /** Hydrates only selected banner collections and retains failed links for explicit retry. */
  async function loadDeferredBanners(): Promise<void> {
    if (bannersLoading || !definition) return
    bannersLoading = true
    const revision = latestRequestId.value
    const sourceOrder = [...definition.bannerSources]
    const selected = base.deferredBanners.filter((collection) => collection.link)
    await Promise.all(selected.map((collection) => limited(async () => {
      let succeeded = false
      const result = await loadDeferredBannerPages({ ...emptyCatalog(), deferredBanners: [collection] }, () => { succeeded = true })
      if (revision !== latestRequestId.value) return
      if (!succeeded) { report(`banners:${collection.source}`, new Error(`Banner load failed: ${collection.source}`)); return }
      issues.value = issues.value.filter((issue) => issue.id !== `banners:${collection.source}`)
      base = { ...base, banners: { ...base.banners, entries: interleaveBanners(sourceOrder.map((id) =>
        [...base.banners.entries, ...result.banners.entries].filter((banner) => banner.source === id))) },
      deferredBanners: base.deferredBanners.filter((item) => item !== collection) }
      publish()
    })))
    if (revision === latestRequestId.value) bannersLoading = false
  }

  /** @param section Render group. @returns Its independently owned members, including subsection overlays. */
  function selectedMembers(section: HomeSection): CustomHomeContribution[] {
    return members.value.filter((member) => section.customContributionIds?.includes(member.id))
  }

  /** @param section Render group. @param initial Whether to fetch only missing first pages. */
  async function loadSection(section: HomeSection, initial = false): Promise<void> {
    const revision = latestRequestId.value
    const candidates = selectedMembers(section)
    const failedIds = new Set(issues.value.map((issue) => issue.id))
    const selected = !initial && candidates.some((member) => failedIds.has(member.id))
      ? candidates.filter((member) => failedIds.has(member.id)) : candidates
    if (initial && !section.customSubsectionKey && selected.some((member) => member.section.subsections?.length)) {
      for (const member of selected) {
        for (const variant of member.section.subsections ?? []) {
          if (revision !== latestRequestId.value) return
          await loadSection({ ...section, customContributionIds: [member.id], customSubsectionKey: variant.key }, true)
        }
      }
      return
    }
    await Promise.all(selected.map(async (member) => {
      if (loadingIds.value.has(member.id)) return
      loadingIds.value = new Set([...loadingIds.value, member.id])
      try {
        const variant = member.section.subsections?.find((item) => item.key === section.customSubsectionKey)
        const target = variant ? { ...member.section, ...variant } : member.section
        const sources = target.sources.filter((source) => initial ? !source.hasInitialItems : !source.hasInitialItems || source.haveMore)
        if (sources.length) {
          let next = target
          const failures: string[] = []
          for (const source of sources) {
            try {
              if (revision !== latestRequestId.value) return
              const cursor = source.hasInitialItems ? source.currentPage + 1 : 1
              const key = JSON.stringify([source.name, source.link, Object.entries(source.params).sort(([a], [b]) => a.localeCompare(b)), cursor])
              const response = await limited(() => revision === latestRequestId.value
                ? requestOnce(`section:${key}`, () => loadHomeSectionPage({ ...target, items: [], sources: [source] }, [source], true))
                : Promise.resolve(target))
              if (revision !== latestRequestId.value) return
              next = { ...next, items: dedupeMediaItems([...next.items, ...response.items]),
                sources: next.sources.map((item) => item === source ? response.sources[0]! : item) }
              next.haveMore = next.sources.some((item) => item.haveMore)
              members.value = members.value.map((item) => item.id !== member.id ? item : {
                ...item, section: variant ? { ...item.section, subsections: item.section.subsections?.map((subsection) =>
                  subsection.key === variant.key ? { ...subsection, ...next, key: variant.key } : subsection) } : next,
              })
              publish()
            } catch (error) {
              failures.push(error instanceof Error ? error.message : String(error))
            }
          }
          if (failures.length) throw new Error(failures.join('\n'))
        }
        if (!initial && member.categoryPage && categories.get(member.blockId)?.haveMore) {
          await loadCategoryNext(member.blockId, revision)
        }
        if (revision === latestRequestId.value) issues.value = issues.value.filter((issue) => issue.id !== member.id)
      } catch (error) { if (revision === latestRequestId.value) report(member.id, error) }
      finally {
        if (revision === latestRequestId.value) {
          loadingIds.value = new Set([...loadingIds.value].filter((id) => id !== member.id))
          publish()
        }
      }
    }))
  }

  /** @param blockId Category block owner. @param revision Active request guard. */
  async function loadCategoryNext(blockId: string, revision: number): Promise<void> {
    const state = categories.get(blockId)
    if (!state || loadingIds.value.has(`category:${blockId}`)) return
    loadingIds.value = new Set([...loadingIds.value, `category:${blockId}`])
    try {
      const result = await limited(() => revision === latestRequestId.value ? categoryPage(state.category, state.page + 1) : Promise.resolve(emptyCatalog()))
      if (revision !== latestRequestId.value) return
      const block = definition?.blocks.find((item) => item.id === blockId)
      const next = columns(blockId, result.sections, block?.titleOverride ?? null).map((member) => ({ ...member,
        section: { ...member.section, haveMore: member.section.haveMore || Boolean(result.haveMore) } }))
      members.value = members.value.map((member) => {
        if (member.blockId !== blockId) return member
        const column = next.find((item) => item.id === member.id)
        return column ? { ...column, section: { ...column.section,
          sources: column.section.sources.map((source) => member.section.sources.find((previous) =>
            previous.name === source.name && previous.link === source.link &&
            JSON.stringify(Object.entries(previous.params).sort()) === JSON.stringify(Object.entries(source.params).sort())) ?? source),
          items: dedupeMediaItems([...member.section.items, ...column.section.items]) } }
          : { ...member, section: { ...member.section, haveMore: false } }
      })
      const known = new Set(members.value.map((member) => member.id))
      const additions = next.filter((member) => !known.has(member.id))
      const lastIndex = members.value.reduce((last, member, index) => member.blockId === blockId ? index : last, -1)
      members.value = [...members.value.slice(0, lastIndex + 1), ...additions, ...members.value.slice(lastIndex + 1)]
      categories.set(blockId, { ...state, page: state.page + 1, haveMore: Boolean(result.haveMore) || result.sections.some((column) => column.haveMore && !column.sources.length) })
    } finally {
      if (revision === latestRequestId.value) loadingIds.value = new Set([...loadingIds.value].filter((id) => id !== `category:${blockId}`))
    }
  }

  /** @param section Render group. @returns Whether any contribution is loading. */
  function isSectionLoading(section: HomeSection): boolean {
    return selectedMembers(section).some((member) => loadingIds.value.has(member.id) || loadingIds.value.has(`category:${member.blockId}`))
  }
  /** @param section Render group. @returns Contextual errors for only its failed members. */
  function getSectionLoadError(section: HomeSection): string | null {
    return issues.value.filter((issue) => section.customContributionIds?.includes(issue.id)).map((issue) => issue.message).join('\n') || null
  }
  /** @param id Failed selection/member. Retries hydration locally, or rediscovers selected references. */
  async function retryIssue(id: string): Promise<void> {
    if (id.startsWith('banners:') && base.deferredBanners.some((collection) => `banners:${collection.source}` === id)) {
      await loadDeferredBanners(); return
    }
    const member = members.value.find((item) => item.id === id)
    if (member) {
      const section = catalog.value?.sections.find((item) => item.customContributionIds?.includes(id))
      if (section) {
        if (member.section.subsections?.length) {
          for (const variant of member.section.subsections) {
            await loadSection({ ...section, customContributionIds: [id], customSubsectionKey: variant.key })
          }
        } else await loadSection({ ...section, customContributionIds: [id] })
      }
      return
    }
    await loadCatalog()
  }
  watch(() => JSON.stringify(toValue(input)), () => { void loadCatalog() }, { immediate: true })
  onScopeDispose(() => { latestRequestId.value++ })
  return { catalog, isLoading, hasLoaded, errorMessage, latestRequestId, issues: computed(() => issues.value),
    loadCatalog, loadDeferredBanners, loadSection, isSectionLoading, getSectionLoadError, retryIssue }
}