import { t } from '@/i18n'
import { buildHomeContentIdentity, markAmbiguousHomeIdentities } from '@/services/homeContentIdentity'
import type { HomeCatalogData, HomeCategory, HomeCategoryChannel, HomeCategorySource } from '@/types/home'

import {
  buildScopedId,
  firstNonEmptyString,
  isJsonRecord,
  normalizeString,
  readStringParamMap,
  resolveAssetUrl,
  slugify,
} from '../primitives/readers'
import type { JsonRecord } from '../types'

/**
 * Converts one raw backend category into the frontend clickable category model.
 *
 * @param payload Raw backend category payload.
 * @param index Stable fallback index used to build a unique category id.
 * @returns Normalized home category.
 */
export function normalizeHomeCategory(
  payload: unknown,
  index: number,
  inheritedSource: string | null = null,
): HomeCategory {
  const record = isJsonRecord(payload) ? payload : {}
  const label = firstNonEmptyString([record.label]) ?? t('category.defaultLabel', { index: String(index + 1) })
  const source = normalizeHomeCategorySource(record, inheritedSource)
  const mergeKey = buildHomeCategoryMergeKey(firstNonEmptyString([record.key]), label, index)

  return {
    channel: normalizeHomeCategoryChannel(record.channel, source?.name ?? inheritedSource),
    identity: buildHomeContentIdentity(
      source?.name ?? inheritedSource,
      firstNonEmptyString([record.key, record.id]),
      source ?? {},
      firstNonEmptyString([record.label]),
    ),
    id:
      buildScopedId(
        firstNonEmptyString([record.id, record.key]) ?? mergeKey,
        inheritedSource,
      ) ??
      `category-${index + 1}`,
    label,
    imageUrl: firstNonEmptyString([record.image_url, record.imageUrl, record.image]),
    mergeKey,
    sources: source ? [source] : [],
  }
}

/**
 * Converts the optional backend channel attribution attached to one category.
 *
 * @param value Raw `channel` value returned with the category.
 * @param source Source id used to resolve a relative logo URL, when available.
 * @returns Normalized channel branding, or null when no display name is usable.
 */
function normalizeHomeCategoryChannel(value: unknown, source: string | null): HomeCategoryChannel | null {
  if (!isJsonRecord(value)) {
    return null
  }

  const name = firstNonEmptyString([value.name, value.label])

  const logo = firstNonEmptyString([value.logo, value.logo])

  return name ? { name, logo: resolveAssetUrl(logo, source) } : null
}

/**
 * Builds a case-, spacing- and accent-insensitive key from a display label.
 *
 * @param label Display label to normalize.
 * @returns Normalized label key, or an empty string when no label is usable.
 */
export function buildCategoryLabelKey(label: string): string {
  const normalizedLabel = normalizeString(label)
  if (!normalizedLabel) return ''

  return normalizedLabel
    .toLocaleLowerCase()
    .normalize('NFD')
    .replace(/\p{Diacritic}/gu, '')
    .replace(/\s+/g, ' ')
}

/**
 * Builds the channel part of the category display grouping key.
 *
 * @param channel Optional channel attribution carried by a category.
 * @returns Normalized channel identity, or an empty string when no channel is present.
 */
function buildCategoryChannelKey(channel: HomeCategoryChannel | null | undefined): string {
  if (!channel) return ''
  return buildCategoryLabelKey(channel.name) || buildCategoryLabelKey(channel.logo ?? '')
}

/**
 * Builds the display grouping key of one category across sources.
 *
 * @param category Normalized home category.
 * @returns Key combining the normalized label and channel identity.
 */
function buildCategoryDisplayGroupKey(category: HomeCategory): string {
  const labelKey = buildCategoryLabelKey(category.label) || category.mergeKey.toLocaleLowerCase()
  return JSON.stringify([labelKey, buildCategoryChannelKey(category.channel)])
}

/**
 * Merges categories that share the same display name across multiple sources.
 * Channel-attributed categories remain distinct when their channel branding differs.
 *
 * @param categories Normalized categories collected from one or more sources.
 * @returns Categories merged by normalized label and channel while preserving first-seen order.
 */
export function mergeHomeCategories(categories: HomeCategory[]): HomeCategory[] {
  const categoriesByGroupKey = new Map<string, HomeCategory>()
  const mergedCategories: HomeCategory[] = []

  categories.forEach((category) => {
    const groupKey = buildCategoryDisplayGroupKey(category)
    const existingCategory = categoriesByGroupKey.get(groupKey)
    if (!existingCategory) {
      const mergedCategory: HomeCategory = {
        ...category,
        sources: dedupeHomeCategorySources(category.sources),
      }

      categoriesByGroupKey.set(groupKey, mergedCategory)
      mergedCategories.push(mergedCategory)
      return
    }

    existingCategory.label ||= category.label
    existingCategory.imageUrl ??= category.imageUrl
    if (category.origins?.length) {
      const origins = [...(existingCategory.origins ?? []), ...category.origins]
      existingCategory.origins = [...new Map(origins.map((origin) => [JSON.stringify(origin), origin])).values()]
    }
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
export function normalizeHomeCategorySource(
  record: JsonRecord,
  inheritedSource: string | null,
): HomeCategorySource | null {
  const source = readStringParamMap(record.source)

  if (Object.keys(source).length > 0) {
    return !source.name && source.source ? { ...source, name: source.source } : source
  }

  if (!inheritedSource) {
    return null
  }

  return {
    name: inheritedSource,
    ...readStringParamMap(record.request),
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
export function buildHomeCategoryMergeKey(rawKey: string | null, label: string, index: number): string {
  const normalizedKey = normalizeString(rawKey ?? '')?.toLocaleLowerCase()
  if (normalizedKey) {
    return normalizedKey
  }

  return slugify(label) || `category-${index + 1}`
}

/**
 * Deduplicates category source descriptors while preserving their first-seen order.
 *
 * @param sources Category source descriptors attached to one merged category.
 * @returns Deduplicated source descriptors.
 */
export function dedupeHomeCategorySources(sources: HomeCategorySource[]): HomeCategorySource[] {
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
export function serializeHomeCategorySource(source: HomeCategorySource): string {
  return JSON.stringify(
    Object.entries(source).sort(([left], [right]) => left.localeCompare(right)),
  )
}

/** @param source Source descriptor. @returns Its request identity excluding the page supplied by get_category. */
export function categoryRequestKey(source: HomeCategorySource): string {
  const { name, source: alias, page: _page, ...request } = source
  const sourceId = name || alias
  return serializeHomeCategorySource({
    ...request,
    ...(sourceId ? { name: sourceId } : {}),
    ...(name && alias && name !== alias ? { source: alias } : {}),
  })
}

/**
 * Coalesces repeated references to the same category and preserves actual identity conflicts.
 * @param categories Categories published in a single lookup scope.
 * @returns Unique category entries with identity collisions marked.
 */
export function identifyCatalogCategories(categories: HomeCategory[]): HomeCategory[] {
  const seen = new Set<string>()
  const unique = categories.filter((category) => {
    if (!category.identity) return true
    const { sourceId, key } = category.identity
    const descriptor = JSON.stringify([sourceId, key, category.sources.map(categoryRequestKey)])
    if (seen.has(descriptor)) return false
    seen.add(descriptor)
    return true
  })
  markAmbiguousHomeIdentities(unique)
  return unique
}

/** @param catalog Home or category page. @returns Categories explicitly targeted by its inline media cards and banners. */
export function catalogCategoryCards(catalog: HomeCatalogData): HomeCategory[] {
  return [
    ...catalog.banners.entries.flatMap((banner) => banner.target?.category ? [banner.target.category] : []),
    ...catalog.sections.flatMap((section) => [section, ...(section.subsections ?? [])]
      .flatMap((rail) => rail.items.flatMap((item) => item.category ? [item.category] : []))),
  ]
}
