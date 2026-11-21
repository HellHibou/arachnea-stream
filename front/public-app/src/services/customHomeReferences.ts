import type { BannerReference, CategoryOrigin, CategoryReference, HomeContentIdentity, SectionReference } from '@/types/customHome'
import type { HomeCategory, HomeSection } from '@/types/home'

/** Maximum parent references accepted in a category lookup path. */
export const MAX_CATEGORY_ANCESTORS = 8

/** @param reference Saved category. @returns Lookup path from the source home to this category. */
export function categoryOrigin(reference: CategoryReference): CategoryOrigin {
  return { sourceId: reference.sourceId, keys: [...(reference.ancestorKeys ?? []), reference.categoryKey] }
}

/** @param reference Banner contribution. @returns Identifier independent of snapshots or credentials. */
export function bannerReferenceKey(reference: BannerReference): string {
  return typeof reference === 'string' ? JSON.stringify(reference) : JSON.stringify(categoryOrigin(reference))
}

/**
 * Refuses ambiguous/aggregated selections instead of choosing a same-title substitute.
 * @param identity Source discovery identity.
 * @returns Usable identity or a contextual error requiring repair.
 */
function requireIdentity(identity: HomeContentIdentity | undefined): HomeContentIdentity {
  if (!identity || identity.ambiguous) {
    throw new Error('Content has no unambiguous source identity; repair its YAML identity before selecting it')
  }
  return identity
}

/**
 * Creates a detached reference only from an unmerged source category.
 * @param category Category from loadHomeSourceDiscovery, not the aggregated catalog.
 * @param ancestorKeys Verified path to the parent category.
 * @returns Source-owned reference; no content, playback URLs, or mutable state are copied.
 */
export function createCategoryReference(category: HomeCategory, ancestorKeys: string[] = []): CategoryReference {
  const identity = requireIdentity(category.identity)
  if (category.sources.length !== 1 || category.sources[0]?.name !== identity.sourceId) {
    throw new Error('Category references require one exact source descriptor')
  }
  return {
    sourceId: identity.sourceId,
    categoryKey: identity.key,
    ...(ancestorKeys.length ? { ancestorKeys } : {}),
    descriptor: { ...category.sources[0] },
    labelSnapshot: category.label,
    titleOverride: null,
  }
}

/**
 * Creates an inline/deferred section reference independent of preference and display ids.
 * @param section Section from loadHomeSourceDiscovery.
 * @param subsectionKey Optional exact discovery identity key under this parent.
 * @param category Owning category, when the section belongs to get_category.
 * @returns Detached reference, including an optional subsection identity.
 */
export function createSectionReference(section: HomeSection, subsectionKey?: string, category?: CategoryReference): SectionReference {
  const identity = requireIdentity(section.identity)
  if (section.sourceOrder.length !== 1 || section.sourceOrder[0] !== identity.sourceId) {
    throw new Error('Section references require an unmerged source section')
  }
  if (subsectionKey !== undefined) {
    const matches = (section.subsections ?? [])
      .filter((subsection) => subsection.identity?.key === subsectionKey)
    if (matches.length !== 1) throw new Error('Selected subsection is missing or ambiguous')
    requireIdentity(matches[0]?.identity)
  }
  return {
    sourceId: identity.sourceId,
    sectionKey: identity.key,
    ...(category ? { category } : {}),
    ...(subsectionKey === undefined ? {} : { subsectionKey }),
    labelSnapshot: !category || subsectionKey === undefined ? section.label :
      section.subsections?.find((entry) => entry.identity?.key === subsectionKey)?.label ?? section.label,
  }
}