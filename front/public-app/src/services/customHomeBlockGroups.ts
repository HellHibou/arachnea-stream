import type { CustomHomeBlock, CustomHomeDisplayOptions } from '@/types/customHome'
import { moveCustomHomeItem } from '@/services/customHomeDraft'

/** @param title Effective title. @returns NFC key preserving accents and punctuation. */
export function customHomeTitleKey(title: string | null): string {
  return (title ?? '').normalize('NFC').trim().replace(/\s+/gu, ' ').toLowerCase()
}

/** Editor grouping uses saved block labels, without fetching category columns. */
export interface CustomHomeBlockGroup {
  key: string
  title: string | null
  blocks: CustomHomeBlock[]
}

/** @param blocks Ordered draft blocks. @param merge Merge titles when enabled. @returns Groups in first-occurrence order. */
export function groupCustomHomeBlocks(blocks: CustomHomeBlock[], merge: boolean): CustomHomeBlockGroup[] {
  const groups = new Map<string, CustomHomeBlockGroup>()
  for (const block of blocks) {
    const title = block.titleOverride?.trim() || block.reference.labelSnapshot
    const titleKey = customHomeTitleKey(title)
    const key = merge && titleKey ? `title:${titleKey}` : `block:${block.id}`
    const group = groups.get(key) ?? { key, title, blocks: [] }
    group.blocks.push(block)
    groups.set(key, group)
  }
  return [...groups.values()]
}

/**
 * Moves whole groups or members without losing references or sharing state.
 * @param blocks Draft blocks. @param merge Merge setting. @param id Target member id.
 * @param direction Movement. @param wholeGroup Move group rather than member.
 * @returns Contiguous grouped order when merged; ordinary individual order otherwise.
 */
export function moveCustomHomeBlock(blocks: CustomHomeBlock[], merge: boolean, id: string,
  direction: -1 | 1, wholeGroup = false): CustomHomeBlock[] {
  if (!merge) return moveCustomHomeItem(blocks, blocks.findIndex((block) => block.id === id), direction)
  let groups = groupCustomHomeBlocks(blocks, true)
  const index = groups.findIndex((group) => group.blocks.some((block) => block.id === id))
  if (index < 0) return [...blocks]
  if (wholeGroup) groups = moveCustomHomeItem(groups, index, direction)
  else {
    const group = groups[index]!
    group.blocks = moveCustomHomeItem(group.blocks, group.blocks.findIndex((block) => block.id === id), direction)
  }
  return groups.flatMap((group) => group.blocks)
}

/** @param blocks Draft blocks. @param merge Merge setting. @param id Edited member. @param patch Changed options only. @returns Updated members preserving other options. */
export function updateCustomHomeGroupDisplay(blocks: CustomHomeBlock[], merge: boolean,
  id: string, patch: CustomHomeDisplayOptions): CustomHomeBlock[] {
  const group = groupCustomHomeBlocks(blocks, merge).find((entry) => entry.blocks.some((block) => block.id === id))
  const ids = new Set(group?.blocks.map((block) => block.id))
  return blocks.map((block) => ids.has(block.id)
    ? { ...block, displayOptions: { ...block.displayOptions, ...patch } } : block)
}