import { moveCustomHomeItem } from '@/services/customHomeDraft'
import { validateCustomHomeDefinition } from '@/services/customHomeValidation'
import type { CustomHomeDefinition } from '@/types/customHome'

/** A single explicit owner, never a display title or the full merged group. */
export type CustomHomeContentMutation =
  | { kind: 'block'; blockId: string; action: 'remove' | 'up' | 'down' }
  | { kind: 'shortcut'; index: number; action: 'remove' }

/**
 * Builds a detached definition for one owner mutation without touching content, bookmarks or storage.
 * @param definition Snapshot whose revision will be checked when saving.
 * @param mutation Explicit owner action.
 * @returns Validated replacement definition; invalid or boundary movements throw before saving.
 */
export function mutateCustomHomeContent(
  definition: CustomHomeDefinition,
  mutation: CustomHomeContentMutation,
): CustomHomeDefinition {
  if (mutation.kind === 'shortcut') {
    if (!Number.isInteger(mutation.index) || !definition.categoryShortcuts[mutation.index]) {
      throw new Error('Selected custom home shortcut no longer exists')
    }
    return validateCustomHomeDefinition({ ...definition,
      categoryShortcuts: definition.categoryShortcuts.filter((_, index) => index !== mutation.index) })
  }
  const index = definition.blocks.findIndex((block) => block.id === mutation.blockId)
  if (index < 0) throw new Error('Selected custom home block no longer exists')
  if (mutation.action === 'remove') {
    return validateCustomHomeDefinition({ ...definition, blocks: definition.blocks.filter((block) => block.id !== mutation.blockId) })
  }
  const direction = mutation.action === 'up' ? -1 : 1
  if (index + direction < 0 || index + direction >= definition.blocks.length) {
    throw new Error('Selected custom home block cannot move beyond the definition boundary')
  }
  return validateCustomHomeDefinition({ ...definition, blocks: moveCustomHomeItem(definition.blocks, index, direction) })
}