import { dedupeMediaItems, interleaveLists } from '@/services/rustify'
import type { CustomHomeDefinition } from '@/types/customHome'
import type { HomeSection } from '@/types/home'
import { customHomeTitleKey } from '@/services/customHomeBlockGroups'
export { customHomeTitleKey } from '@/services/customHomeBlockGroups'

/** Runtime-only member with independently owned section or category pagination. */
export interface CustomHomeContribution {
  id: string
  blockId: string
  section: HomeSection
  categoryPage: boolean
}

/**
 * Composes selected members without sharing cursors or using titles as persistent identities.
 * @param definition Validated home definition.
 * @param contributions Members in definition/block/column order.
 * @returns Render groups positioned at their first contribution, with home-scoped preferences.
 */
export function composeCustomHomeSections(
  definition: CustomHomeDefinition,
  contributions: CustomHomeContribution[],
): HomeSection[] {
  const groups = new Map<string, CustomHomeContribution[]>()
  for (const contribution of contributions) {
    const titleKey = customHomeTitleKey(contribution.section.label)
    const groupKey = definition.sectionMergeMode === 'title' && titleKey &&
      !contribution.section.subsections?.length
      ? `title:${titleKey}` : `member:${contribution.id}`
    const group = groups.get(groupKey) ?? []
    group.push(contribution)
    groups.set(groupKey, group)
  }
  return [...groups.values()].map((members) => {
    const first = members[0]!
    const id = JSON.stringify(members.map((member) => member.id))
    return {
      ...first.section,
      customDisplayOptions: definition.blocks.find((block) => block.id === first.blockId)?.displayOptions,
      id,
      preferenceKey: `custom:${encodeURIComponent(definition.id)}:${encodeURIComponent(id)}`,
      items: dedupeMediaItems(interleaveLists(members.map((member) => member.section.items))),
      sources: [],
      sourceOrder: members.flatMap((member) => member.section.sourceOrder),
      haveMore: members.some((member) => member.section.haveMore ||
        member.section.sources.some((source) => !source.hasInitialItems || source.haveMore)),
      customContributionIds: members.map((member) => member.id),
      customBlockIds: [...new Set(members.map((member) => member.blockId))],
      customNeedsInitial: members.some((member) =>
        [...member.section.sources, ...(member.section.subsections ?? []).flatMap((variant) => variant.sources)]
          .some((source) => !source.hasInitialItems)),
    }
  })
}