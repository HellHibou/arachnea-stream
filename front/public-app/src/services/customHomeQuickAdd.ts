import { loadHomeSourceDiscovery } from '@/services/rustify'
import { createUuid } from '@/utils'
import { createCategoryReference, createSectionReference } from '@/services/customHomeReferences'
import { applyCustomHomeSelection } from '@/services/customHomeDraft'
import type { CustomHomeAddIntent, CustomHomeDefinition, CustomHomeSelection } from '@/types/customHome'

/** @param descriptor Category request identity. @returns Stable descriptor comparison independent of field order. */
function descriptorKey(descriptor: Record<string, string>): string {
  return JSON.stringify(Object.entries(descriptor).sort(([left], [right]) => left.localeCompare(right)))
}

/**
 * Resolves every contribution of the clicked content, without picker/editor or deferred hydration.
 * @param intent Clicked banner collection, category or section.
 * @param categoryMode Category addition as shortcut or embedded section.
 * @returns Detached selections; ambiguous or missing contributions reject the whole addition.
 */
export async function resolveQuickAddSelections(
  intent: CustomHomeAddIntent,
  categoryMode: 'shortcut' | 'section',
): Promise<CustomHomeSelection[]> {
  if (intent.kind === 'categories') {
    if (!intent.categories.length) throw new Error('Select at least one category')
    const selections: CustomHomeSelection[] = []
    for (const category of intent.categories) {
      selections.push(...await resolveQuickAddSelections({ kind: 'category', category }, categoryMode))
    }
    return selections
  }
  if (intent.kind === 'banners') return [{ kind: 'banners', sourceId: intent.sourceId }]
  if (intent.kind === 'category' && intent.category.identity && intent.category.sources.length === 1) {
    const reference = createCategoryReference(intent.category)
    return [categoryMode === 'shortcut' ? { kind: 'shortcut', reference } : {
      kind: 'block', block: { id: createUuid(), kind: 'category-content', reference, titleOverride: null },
    }]
  }
  if (intent.kind === 'section' && intent.section.identity && intent.section.sourceOrder.length === 1) {
    return [{ kind: 'block', block: { id: createUuid(), kind: 'home-section',
      reference: createSectionReference(intent.section), titleOverride: null } }]
  }
  const sourceIds = intent.kind === 'category'
    ? [...new Set(intent.category.sources.map((source) => source.name).filter((id): id is string => Boolean(id)))]
    : [...new Set(intent.section.sourceOrder)]
  if (!sourceIds.length) throw new Error('Selected content has no source contributions')
  const discoveries = await loadHomeSourceDiscovery(sourceIds)
  return sourceIds.flatMap((sourceId): CustomHomeSelection[] => {
    const row = discoveries.find((discovery) => discovery.sourceId === sourceId)
    if (!row?.available) throw new Error(`Source unavailable: ${sourceId}`)
    if (intent.kind === 'category') {
      const descriptors = intent.category.sources.filter((source) => source.name === sourceId)
      return descriptors.map((descriptor) => {
        const matches = row.catalog.categories.filter((category) =>
          category.sources.some((source) => descriptorKey(source) === descriptorKey(descriptor)))
        if (matches.length !== 1) throw new Error(`Selected category is missing or ambiguous: ${sourceId}`)
        const reference = createCategoryReference(matches[0]!)
        return categoryMode === 'shortcut' ? { kind: 'shortcut', reference } : {
          kind: 'block', block: { id: createUuid(), kind: 'category-content', reference, titleOverride: null },
        }
      })
    }
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