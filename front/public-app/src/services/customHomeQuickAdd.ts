import { categorySectionReferences, createCategoryCatalogResolver, withCategoryOrigins, type CategoryContext } from '@/services/customHomeCategoryResolver'
import { t } from '@/i18n'
import { catalogCategoryCards, categoryRequestKey, identifyCatalogCategories } from '@/services/rustify/home/categories'
import { loadHomeSourceDiscovery } from '@/services/rustify'
import { createUuid } from '@/utils'
import { createCategoryReference, createSectionReference } from '@/services/customHomeReferences'
import { applyCustomHomeSelection } from '@/services/customHomeDraft'
import type { HomeCategory } from '@/types/home'
import type { CategoryReference, CustomHomeAddIntent, CustomHomeCategoryMode, CustomHomeDefinition, CustomHomeSelection } from '@/types/customHome'

/**
 * @param categories Clicked categories with optional navigation hints.
 * @param resolver Catalog discovery shared by this navigation or add operation.
 * @returns Unambiguous references proved by replaying the category's origin.
 */
export async function discoverCategoryReferences(
  categories: HomeCategory[], resolver = createCategoryCatalogResolver(),
): Promise<CategoryReference[]> {
  if (!categories.length) throw new Error('Select at least one category')
  const references = await Promise.all(categories.flatMap((category) => {
    if (!category.sources.length) throw new Error('Selected category has no source contributions')
    return category.sources.map(async (descriptor) => {
      const sourceId = descriptor.name || descriptor.source
      if (!sourceId) throw new Error('Selected category has no source identifier')
      const descriptorKey = categoryRequestKey(descriptor)
      const matchesDescriptor = (candidate: HomeCategory) =>
        candidate.sources.some((source) => categoryRequestKey(source) === descriptorKey)
      const hinted = await Promise.allSettled((category.origins ?? [])
        .filter((origin) => origin.sourceId === sourceId)
        .map(async (origin) => ({ origin, category: await resolver.resolve(origin) })))
      const matches = hinted.flatMap((entry) =>
        entry.status === 'fulfilled' && matchesDescriptor(entry.value.category) ? [entry.value] : [])
      if (matches.length === 1) {
        const match = matches[0]!
        return createCategoryReference(match.category, match.origin.keys.slice(0, -1))
      }
      if (!matches.length) {
        const home = await resolver.source(sourceId)
        const direct = home.categories.filter(matchesDescriptor)
        if (direct.length === 1) return createCategoryReference(direct[0]!)
        if (!direct.length) {
          const cards = identifyCatalogCategories(catalogCategoryCards(home)).filter(matchesDescriptor)
          if (cards.length === 1) return createCategoryReference(cards[0]!)
        }
      }
      const failure = hinted.find((entry) => entry.status === 'rejected')
      if (!matches.length && failure?.status === 'rejected') throw failure.reason
      throw new Error(`Selected category is missing or ambiguous: ${sourceId}`)
    })
  }))
  return references
}

/**
 * @param category Category shown in the current route.
 * @param sourceIds Sources contributing to the requested content.
 * @param resolver Shared discovery for this addition.
 * @returns Verified unmerged catalogs for the relevant category contributions.
 */
async function loadCategoryContexts(
  category: HomeCategory, sourceIds: string[], resolver: ReturnType<typeof createCategoryCatalogResolver>,
): Promise<CategoryContext[]> {
  const sources = category.sources.filter((source) => sourceIds.includes(source.name || source.source || ''))
  let references: CategoryReference[]
  try {
    references = await discoverCategoryReferences([{ ...category, sources }], resolver)
  } catch {
    throw new Error(t('customHome.categoryOriginUnavailable'))
  }
  try {
    return await Promise.all(references.map(async (reference) => ({
      reference, catalog: (await resolver.load(reference)).catalog,
    })))
  } catch {
    throw new Error(t('customHome.categoryCatalogUnavailable'))
  }
}

/**
 * Resolves every contribution of the clicked content before saving.
 * @param intent Clicked banner collection, category or home section.
 * @param categoryMode Category addition as shortcut or embedded content.
 * @param supportsCategory Whether a source advertises get_category.
 * @returns Detached selections; ambiguous or missing contributions reject the whole addition.
 */
export async function resolveQuickAddSelections(
  intent: CustomHomeAddIntent,
  categoryMode: CustomHomeCategoryMode,
  supportsCategory: (sourceId: string) => boolean,
): Promise<CustomHomeSelection[]> {
  if (intent.kind === 'category' || intent.kind === 'categories') {
    const categories = intent.kind === 'category' ? [intent.category] : intent.categories
    if (categoryMode === 'content' && categories.some((category) => category.sources.some((source) =>
      !source.name || !supportsCategory(source.name)))) {
      throw new Error(t('customHome.categoryContentUnavailable'))
    }
    const resolver = createCategoryCatalogResolver()
    const parent = intent.kind === 'categories' ? intent.parent : undefined
    let resolvedCategories = categories
    if (parent && categories.length) {
      const sourceIds = categories.flatMap((category) => category.sources.flatMap((source) => source.name ? [source.name] : []))
      const contexts = await loadCategoryContexts(parent, sourceIds, resolver)
      resolvedCategories = categories.map((category) => withCategoryOrigins(category, contexts))
    }
    const references = await discoverCategoryReferences(resolvedCategories, resolver)
    return references.map((reference) => categoryMode === 'shortcut' ? { kind: 'shortcut', reference } : {
      kind: 'block', block: { id: createUuid(), kind: 'category-content', reference, titleOverride: null },
    })
  }
  if (intent.kind === 'category-banners') {
    const contexts = await loadCategoryContexts(intent.category, [intent.sourceId], createCategoryCatalogResolver())
    const collections = contexts.filter(({ catalog }) =>
      catalog.banners.entries.length || catalog.deferredBanners.some((collection) => collection.link))
    if (!collections.length) throw new Error(t('customHome.categoryBannersUnavailable'))
    return collections.map(({ reference }) => ({ kind: 'category-banners', reference }))
  }
  if (intent.kind === 'category-section') {
    const { category, section, subsectionKey } = intent
    const sources = subsectionKey === undefined ? section.sourceOrder :
      section.subsections?.find((subsection) => subsection.key === subsectionKey)?.sourceOrder ?? []
    const contexts = await loadCategoryContexts(category, sources, createCategoryCatalogResolver())
    const references = categorySectionReferences(contexts, section, subsectionKey)
    if (!references.length) throw new Error(t('customHome.categorySectionUnavailable'))
    return references.map((reference) => ({
      kind: 'block', block: { id: createUuid(), kind: 'home-section', reference, titleOverride: null },
    }))
  }
  if (intent.kind === 'banners') return [{ kind: 'banners', sourceId: intent.sourceId }]
  if (intent.kind === 'section' && intent.section.identity && intent.section.sourceOrder.length === 1) {
    return [{ kind: 'block', block: { id: createUuid(), kind: 'home-section',
      reference: createSectionReference(intent.section), titleOverride: null } }]
  }
  const sourceIds = [...new Set(intent.section.sourceOrder)]
  if (!sourceIds.length) throw new Error('Selected content has no source contributions')
  const discoveries = await loadHomeSourceDiscovery(sourceIds)
  return sourceIds.flatMap((sourceId): CustomHomeSelection[] => {
    const row = discoveries.find((discovery) => discovery.sourceId === sourceId)
    if (!row?.available) throw new Error(`Source unavailable: ${sourceId}`)
    const prefix = `source:${encodeURIComponent(sourceId)}:`
    const preferenceKey = intent.section.preferenceKey.startsWith(prefix)
      ? intent.section.preferenceKey.slice(prefix.length) : intent.section.preferenceKey
    const matches = row.catalog.sections.filter((section) => section.preferenceKey === preferenceKey)
    if (matches.length === 0) throw new Error(`Selected section is missing or ambiguous: ${sourceId}`)
    return matches.map((section) => ({ kind: 'block', block: {
      id: createUuid(), kind: 'home-section', reference: createSectionReference(section), titleOverride: null,
    } }))
  })
}

/** @param draft Detached destination. @param selections Fully resolved contributions. @returns Definition saved as one record. */
export function applyQuickAddSelections(draft: CustomHomeDefinition, selections: CustomHomeSelection[]): CustomHomeDefinition {
  return selections.reduce((value, selection) => applyCustomHomeSelection(value, selection), draft)
}