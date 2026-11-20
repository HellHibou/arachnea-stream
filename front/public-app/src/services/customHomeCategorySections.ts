import { mediaRouteIdentity } from '@/services/mediaRouting'
import { dedupeMediaItems } from '@/services/rustify'
import type { HomeCatalogData, HomeSection, HomeSectionSubsection } from '@/types/home'
import type { CustomHomeContribution } from './customHomeComposition'

type CategoryRail = Pick<HomeSection, 'items' | 'sources' | 'label' | 'identity' | 'haveMore'>

/** @param rail Category section or subsection. @returns Whether it can render cards or fetch them through its link. */
export function hasCategoryRailContent(rail: CategoryRail): boolean {
  return rail.items.length > 0 || rail.sources.some((source) => !source.hasInitialItems || source.haveMore)
}

/** @param catalog Category response. @returns Whether the category itself has another page. */
export function hasMoreCategoryPages(catalog: HomeCatalogData): boolean {
  return Boolean(catalog.haveMore) || catalog.sections.some((section) =>
    [section, ...(section.subsections ?? [])].some((rail) => rail.haveMore && !rail.sources.length))
}

/** @param rail Incoming rail. @param candidates Rails on one page. @returns An unambiguous match within that page. */
function findRail<T extends CategoryRail>(rail: CategoryRail, candidates: T[]): T | undefined {
  const matches = rail.identity ? candidates.filter((candidate) =>
    candidate.identity?.key === rail.identity?.key && candidate.identity?.sourceId === rail.identity?.sourceId) : []
  if (matches.length > 0 || rail.identity?.basis === 'provider' || !rail.label) {
    return matches.length === 1 ? matches[0] : undefined
  }

  const byLabel = candidates.filter((candidate) => candidate.label === rail.label &&
    candidate.identity?.basis !== 'provider')
  return byLabel.length === 1 ? byLabel[0] : undefined
}

/** @param current Accumulated rail. @param incoming Matching rail from a new category page. @returns Rail with its own cursor preserved. */
function appendRail<T extends CategoryRail>(current: T, incoming: T): T {
  const sources = [
    ...current.sources,
    ...incoming.sources.filter((source) => !current.sources.some((previous) => previous.name === source.name)),
  ]
  return { ...current, items: dedupeMediaItems([...current.items, ...incoming.items]), sources,
    haveMore: sources.some((source) => source.haveMore) }
}

/** @param current Accumulated section. @param incoming Matching section. @returns The same rail and subsection identities with appended cards. */
function appendSection(current: HomeSection, incoming: HomeSection): HomeSection {
  const previous = current.subsections ?? []
  const subsections = [...previous]
  const received = incoming.subsections ?? []
  for (const subsection of received) {
    const match = findRail(subsection, previous)
    if (match && findRail(match, received) === subsection) {
      subsections[previous.indexOf(match)] = appendRail(match, subsection)
    } else {
      let key = subsection.key
      while (subsections.some((candidate) => candidate.key === key)) key += ':next'
      subsections.push({ ...subsection, key })
    }
  }
  return { ...appendRail(current, incoming), subsections: subsections.length ? subsections : undefined }
}

/**
 * Appends one category page without associating unnamed rails by their position on different pages.
 * @param blockId Owning category block.
 * @param previous Accumulated sections including independently loaded pages.
 * @param sections Sections from the incoming category page.
 * @param page Requested page, used to scope anonymous or ambiguous display identities.
 * @returns Contributions in first-seen section order.
 */
export function appendCategorySections(
  blockId: string, previous: CustomHomeContribution[], sections: HomeSection[], page: number,
): CustomHomeContribution[] {
  const previousSections = previous.map((member) => member.section)
  const members = [...previous]
  const usedIds = new Set(previous.map((member) => member.id))
  for (const [index, rawSection] of sections.entries()) {
    const section = {
      ...rawSection,
      items: rawSection.items.filter(mediaRouteIdentity),
      subsections: rawSection.subsections?.map((subsection) => ({
        ...subsection, items: subsection.items.filter(mediaRouteIdentity),
      })),
    }
    const match = findRail(rawSection, previousSections)
    if (match && findRail(match, sections) === rawSection) {
      const position = previousSections.indexOf(match)
      const member = previous[position]!
      members[position] = { ...member, section: appendSection(match, section) }
    } else {
      const stableId = section.identity && JSON.stringify([blockId, section.identity.key])
      const id = stableId && !usedIds.has(stableId) && findRail(rawSection, sections) === rawSection
        ? stableId : JSON.stringify([blockId, page, index])
      usedIds.add(id)
      members.push({ id, blockId, section })
    }
  }
  return members
}

/** @param section Accumulated category section. @returns Displayable subsection rails in category order. */
export function categorySubsections(section: HomeSection): HomeSectionSubsection[] {
  return (section.subsections ?? []).filter(hasCategoryRailContent).map((subsection) => ({
    ...subsection, haveMore: subsection.sources.some((source) => source.haveMore),
  }))
}
