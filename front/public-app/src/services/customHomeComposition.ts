import { dedupeMediaItems, interleaveLists } from '@/services/rustify'
import type { CustomHomeDefinition } from '@/types/customHome'
import type { HomeSection } from '@/types/home'
import { categorySubsections, hasCategoryRailContent } from '@/services/customHomeCategorySections'
import { customHomeTitleKey } from '@/services/customHomeBlockGroups'
export { customHomeTitleKey } from '@/services/customHomeBlockGroups'

/** Runtime-only member with independently owned section or category pagination. */
export interface CustomHomeContribution {
  id: string
  blockId: string
  section: HomeSection
}

/**
 * Composes selected members without sharing cursors or using titles as persistent identities.
 * @param definition Validated home definition.
 * @param contributions Members in definition/block/column order.
 * @param categoryLabels Current labels for resolved category blocks.
 * @returns Render groups positioned at their first contribution, with home-scoped preferences.
 */
export function composeCustomHomeSections(
  definition: CustomHomeDefinition,
  contributions: CustomHomeContribution[],
  categoryLabels: ReadonlyMap<string, string>,
): HomeSection[] {
  const groups = new Map<string, CustomHomeContribution[]>()
  const blocks = new Map(definition.blocks.map((block) => [block.id, block]))
  const categoryCounts = new Map<string, number>()
  const visible = contributions.flatMap((member) => {
    const block = blocks.get(member.blockId)
    if (!block || (block.kind === 'home-section' && !block.reference.category)) return [member]
    const subsections = categorySubsections(member.section)
    if (!hasCategoryRailContent(member.section) && !subsections.length) return []

    categoryCounts.set(member.blockId, (categoryCounts.get(member.blockId) ?? 0) + 1)
    return [{ ...member, section: { ...member.section, subsections,
      haveMore: member.section.sources.some((source) => source.haveMore) } }]
  })
  for (const contribution of visible) {
    const block = blocks.get(contribution.blockId)
    const category = block?.kind === 'category-content'
    const multiSection = (categoryCounts.get(contribution.blockId) ?? 0) > 1
    const titleOverride = multiSection ? null : block?.titleOverride?.trim()
    const label = titleOverride || contribution.section.label ||
      (category ? categoryLabels.get(contribution.blockId) ?? block.reference.labelSnapshot : null)
    const member = { ...contribution, section: { ...contribution.section, label } }
    const titleKey = customHomeTitleKey(label)
    const merge = definition.sectionMergeMode === 'title' && titleKey && !multiSection &&
      !member.section.subsections?.length
    const groupKey = merge ? `title:${titleKey}` : `member:${member.id}`
    const group = groups.get(groupKey) ?? []
    group.push(member)
    groups.set(groupKey, group)
  }
  return [...groups.values()].map((members) => {
    const first = members[0]!
    const id = JSON.stringify(members.map((member) => member.id))
    return {
      ...first.section,
      customDisplayOptions: blocks.get(first.blockId)?.displayOptions,
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
