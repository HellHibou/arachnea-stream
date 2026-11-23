import { getCategoryCatalog, loadHomeSourceDiscovery } from '@/services/rustify'
import { useServiceMetadata } from '@/composables/useServiceMetadata'
import { categoryOrigin, createCategoryReference, createSectionReference, MAX_CATEGORY_ANCESTORS } from '@/services/customHomeReferences'
import type { CategoryOrigin, CategoryReference, HomeSourceDiscovery, SectionReference } from '@/types/customHome'
import { catalogCategoryCards, categoryRequestKey, identifyCatalogCategories, normalizeHomeCategory } from '@/services/rustify/home/categories'
import type { HomeCatalogData, HomeCategory, HomeCategorySource, HomeSection, HomeSectionSubsection } from '@/types/home'
import type { ServiceMetadata } from '@/types/serviceMetadata'

/**
 * Resolves a category directly when its service declares support for the exact URL.
 * @param source Category source descriptor.
 * @param label Category display name.
 * @param service Metadata for the descriptor's source.
 * @returns The category with only the validated URL forwarded to the scraper.
 */
function directCategory(
  source: HomeCategorySource, label: string, service: ServiceMetadata | null,
): HomeCategory | undefined {
  const name = source.name
  const url = source.query_url
  if (!name || !url || service?.id !== name || (source.source && source.source !== name) ||
    !service.supportedQueries.includes('get_category')) return undefined

  const supported = service.categoryUrlPatterns?.some((pattern) => {
    try {
      return new RegExp(pattern).exec(url)?.[0] === url
    } catch {
      return false
    }
  })
  if (!supported) return undefined

  const category = normalizeHomeCategory({ label, request: { query_url: url } }, 0, name)
  if (category.identity) category.identity.key = `card:${category.identity.key}`
  return category
}

/**
 * Shares catalog discovery for one navigation or custom-home load. Failed requests remain retryable.
 * @param loadSource Loads a fresh catalog from one service.
 * @param loadPage Loads an unmerged first category page.
 */
export function createCategoryCatalogResolver(
  loadSource: (id: string) => Promise<HomeSourceDiscovery | undefined> = async (id) => (await loadHomeSourceDiscovery([id]))[0],
  loadPage: (category: HomeCategory) => Promise<HomeCatalogData> = (category) => getCategoryCatalog(category, 1, false, true),
) {
  const metadata = useServiceMetadata()
  const sources = new Map<string, Promise<HomeCatalogData>>()
  const pages = new Map<string, Promise<HomeCatalogData>>()

  /** @param descriptor Category source descriptor. @param label Display label. @returns Category if its URL is configured for direct lookup. */
  function fromUrl(descriptor: HomeCategorySource, label: string): HomeCategory | undefined {
    return directCategory(descriptor, label, metadata.getService(descriptor.name))
  }

  /** @param reference Saved category. @returns Category if its URL is configured for direct lookup. */
  function directReference(reference: CategoryReference): HomeCategory | undefined {
    return fromUrl(reference.descriptor, reference.labelSnapshot)
  }

  /** @param id Source identifier. @returns Its unmerged home or an explicit discovery failure. */
  async function source(id: string): Promise<HomeCatalogData> {
    let request = sources.get(id)
    if (!request) {
      request = loadSource(id).then((result) => {
        if (!result?.available) throw new Error(`Source unavailable: ${id}`)
        return result.catalog
      }).catch((error) => { sources.delete(id); throw error })
      sources.set(id, request)
    }
    return request
  }

  /** @param origin Source-owned chain. @param category Rediscovered category. @returns Its unmerged first page. */
  function page(origin: CategoryOrigin, category: HomeCategory): Promise<HomeCatalogData> {
    const key = JSON.stringify([origin, category.sources.map(categoryRequestKey)])
    let request = pages.get(key)
    if (!request) {
      request = loadPage(category).catch((error) => { pages.delete(key); throw error })
      pages.set(key, request)
    }
    return request
  }

  /** @param origin Source and ordered keys. @returns Current category after resolving every parent. */
  async function resolve(origin: CategoryOrigin): Promise<HomeCategory> {
    const { sourceId, keys } = origin
    if (!keys.length || keys.length > MAX_CATEGORY_ANCESTORS + 1) throw new Error('Invalid category path')
    let catalog = await source(sourceId)
    for (const [index, key] of keys.entries()) {
      const candidates = key.startsWith('card:') ? identifyCatalogCategories(catalogCategoryCards(catalog)) : catalog.categories
      const matches = candidates.filter((category) => category.identity?.sourceId === sourceId &&
        category.identity.key === key && !category.identity.ambiguous)
      if (matches.length !== 1) throw new Error(`Category missing or ambiguous: ${key}`)
      const category = matches[0]!
      if (index === keys.length - 1) return { ...category, origins: [origin] }
      catalog = await page({ sourceId, keys: keys.slice(0, index + 1) }, category)
    }
    throw new Error('Invalid category path')
  }

  /** @param reference Saved category. @returns Its current direct or rediscovered category. */
  async function resolveReference(reference: CategoryReference): Promise<HomeCategory> {
    return directReference(reference) ?? await resolve(categoryOrigin(reference))
  }

  /** @param reference Saved category. @param resolved Previously resolved category, if available. @returns Its descriptor and first page. */
  async function load(reference: CategoryReference, resolved?: HomeCategory) {
    const origin = categoryOrigin(reference)
    const category = resolved ?? await resolveReference(reference)
    return { category, catalog: await page(origin, category) }
  }

  /** Clears successful or failed discovery snapshots before a reference repair attempt. */
  function clear(): void {
    sources.clear()
    pages.clear()
  }

  return { source, resolve, resolveReference, fromUrl, directReference, load, clear }
}

/** @param catalog Unmerged owner catalog. @param reference Selected section or variant. @returns The exact source-owned rail. */
export function resolveCatalogSection(catalog: HomeCatalogData, reference: SectionReference): HomeSection | undefined {
  const matches = catalog.sections.filter((section) => section.identity?.sourceId === reference.sourceId &&
    section.identity.key === reference.sectionKey && !section.identity.ambiguous)
  if (matches.length !== 1) return undefined
  const section = matches[0]!
  if (reference.subsectionKey === undefined) return section
  const variants = (section.subsections ?? []).filter((variant) => variant.identity?.key === reference.subsectionKey &&
    !variant.identity?.ambiguous)
  if (variants.length !== 1) return undefined
  const variant = variants[0]!
  return { ...section, ...variant, subsections: undefined }
}

/** Verified reference and unmerged first page of its category. */
export interface CategoryContext {
  reference: CategoryReference
  catalog: HomeCatalogData
}

/** @param category Displayed child. @param contexts Verified parent catalogs. @returns Its verified parent path for each source contribution. */
export function withCategoryOrigins(category: HomeCategory, contexts: CategoryContext[]): HomeCategory {
  const origins = []
  for (const descriptor of category.sources) {
    const key = categoryRequestKey(descriptor)
    const matches = contexts.flatMap(({ reference, catalog }) =>
      catalog.categories.filter((child) => child.identity?.sourceId === reference.sourceId && !child.identity.ambiguous &&
        child.sources.some((source) => categoryRequestKey(source) === key))
        .map((child) => ({ reference, child })))
    if (matches.length !== 1) continue
    const { reference, child } = matches[0]!
    const parent = categoryOrigin(reference)
    if (parent.keys.length <= MAX_CATEGORY_ANCESTORS) {
      origins.push(categoryOrigin(createCategoryReference(child, parent.keys)))
    }
  }
  return origins.length ? { ...category, origins } : category
}

/** @param contexts Verified owner catalogs. @param section Displayed rail. @param variantKey Active subsection, if any. @returns All identifiable contributions. */
export function categorySectionReferences(contexts: CategoryContext[], section: HomeSection, variantKey?: string): SectionReference[] {
  const selected = contexts.flatMap(({ reference, catalog }) =>
    catalog.sections.filter((candidate) => candidate.preferenceKey === section.preferenceKey)
      .flatMap<{ reference: CategoryReference; section: HomeSection; variant?: HomeSectionSubsection }>((candidate) => {
        if (variantKey === undefined) return [{ reference, section: candidate, variant: undefined }]
        return (candidate.subsections ?? []).filter((variant) => variant.key === variantKey)
          .map((variant) => ({ reference, section: candidate, variant }))
      }))
  const sources = variantKey === undefined ? section.sourceOrder :
    section.subsections?.find((variant) => variant.key === variantKey)?.sourceOrder ?? []
  if (!sources.every((source) => selected.some((entry) => entry.reference.sourceId === source))) return []
  try {
    return selected.map((entry) => {
      if (entry.variant && !entry.variant.identity?.key) throw new Error('Subsection has no identity')
      return createSectionReference(entry.section, entry.variant?.identity?.key, entry.reference)
    })
  } catch {
    return []
  }
}
