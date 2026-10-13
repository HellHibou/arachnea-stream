import { validateCustomHomeDefinition } from '@/services/customHomeValidation'
import type { CustomHomeDefinition, CustomHomeSelection } from '@/types/customHome'

/**
 * Applies a picker selection to a detached draft; repeated rails are intentional, banners are unique.
 * @param draft Current detached definition.
 * @param selection Validated source-owned selection.
 * @param repairBlockId Optional block to replace while retaining its position, id and title override.
 * @returns Detached updated definition; no storage writes occur.
 */
export function applyCustomHomeSelection(
  draft: CustomHomeDefinition,
  selection: CustomHomeSelection,
  repairBlockId?: string,
): CustomHomeDefinition {
  let result = draft
  if (selection.kind === 'banners') {
    result = { ...draft, bannerSources: [...new Set([...draft.bannerSources, selection.sourceId])] }
  } else if (selection.kind === 'shortcut') {
    const duplicate = draft.categoryShortcuts.some((reference) =>
      reference.sourceId === selection.reference.sourceId && reference.categoryKey === selection.reference.categoryKey)
    result = duplicate ? draft : { ...draft, categoryShortcuts: [...draft.categoryShortcuts, selection.reference] }
  } else if (repairBlockId) {
    if (!draft.blocks.some((block) => block.id === repairBlockId)) throw new Error('Repair target no longer exists')
    result = { ...draft, blocks: draft.blocks.map((block) => block.id === repairBlockId
      ? { ...selection.block, id: block.id, titleOverride: block.titleOverride } : block) }
  } else result = { ...draft, blocks: [...draft.blocks, selection.block] }
  return validateCustomHomeDefinition(result)
}

/** @param items Ordered values. @param index Selected position. @param direction Accessible movement. @returns Reordered copy. */
export function moveCustomHomeItem<T>(items: T[], index: number, direction: -1 | 1): T[] {
  const target = index + direction
  if (index < 0 || index >= items.length || target < 0 || target >= items.length) return [...items]
  const result = [...items]
  ;[result[index], result[target]] = [result[target]!, result[index]!]
  return result
}