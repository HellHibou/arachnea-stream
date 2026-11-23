import type { Ref } from 'vue'
import { deleteSection, getAllSections, reorderSections, saveSection, type HomeSectionRecord } from '@/services/homeSections'
import { BOOKMARKS_SECTION_PREFERENCE_KEY } from '@/services/entryBookmarks'
import { isCurrentProfile, requireProfileContext, type ProfileContext } from '@/services/profileContext'
import { isUncertainProfileWrite, ProfileApiError } from '@/services/profileApi'
import { reportPersonalDataFailure } from '@/services/personalDataStatus'
import type { MediaCardCollectionMode, ThumbnailImageFit, ThumbnailOrientation } from '@/types/media'
import type { SectionRecordDefaults } from './types'

/** Confirmed section operations consumed by the home catalogue. */
export interface SectionRecordsBridge {
  toggleSectionPinned: (sectionKey: string) => Promise<void>
  movePinnedSection: (sectionKey: string, direction: 'up' | 'down', visibleKeys?: string[]) => Promise<void>
  updateSectionCollectionMode: (sectionKey: string, mode: MediaCardCollectionMode, defaults?: Omit<SectionRecordDefaults, 'collectionMode'>) => Promise<void>
  updateSectionThumbnailOrientation: (sectionKey: string, orientation: ThumbnailOrientation, defaults?: Omit<SectionRecordDefaults, 'thumbnailOrientation'>) => Promise<void>
  updateSectionThumbnailImageFit: (sectionKey: string, fit: ThumbnailImageFit, defaults?: Omit<SectionRecordDefaults, 'thumbnailImageFit'>) => Promise<void>
}

/** Creates serialized server operations; reactive caches change only after confirmation.
 * @param sectionRecords Confirmed cache for the selected profile.
 */
export function createSectionRecordsBridge(sectionRecords: Ref<HomeSectionRecord[]>): SectionRecordsBridge {
  let queue = Promise.resolve()
  let uncertainGeneration: number | null = null
  /** Serializes actions and refuses stale profile callbacks. @param work Mutation to execute. */
  function run(work: (context: ProfileContext) => Promise<void>): Promise<void> {
    let context
    try { context = requireProfileContext() } catch (error) { reportPersonalDataFailure(error); return Promise.reject(error) }
    const result = queue.then(async () => {
      if (!isCurrentProfile(context)) return
      try {
        if (uncertainGeneration === context.generation) {
          const records = await getAllSections(context)
          if (!isCurrentProfile(context)) return
          sectionRecords.value = records
          uncertainGeneration = null
        }
        await work(context)
        const records = await getAllSections(context)
        if (isCurrentProfile(context)) sectionRecords.value = records
      } catch (error) {
        if (isCurrentProfile(context)) {
          if (isUncertainProfileWrite(error) || (error instanceof ProfileApiError && error.code === 'revision_conflict')) {
            uncertainGeneration = context.generation
            const confirmed = await getAllSections(context).catch(() => null)
            if (confirmed && isCurrentProfile(context)) {
              sectionRecords.value = confirmed
              uncertainGeneration = null
            }
          }
          if (isCurrentProfile(context)) reportPersonalDataFailure(error)
        }
        throw error
      }
    })
    queue = result.catch(() => {})
    return result
  }
  /** Gets a confirmed section or a detached create-only default. @param name Profile-local key. @param defaults Display defaults for new records. */
  function draft(name: string, defaults: SectionRecordDefaults = {}): HomeSectionRecord {
    return sectionRecords.value.find((r) => r.name === name) ?? { name, order: Math.max(-1, ...sectionRecords.value.map((record) => record.order)) + 1, collectionMode: defaults.collectionMode ?? 'single-row', thumbnailOrientation: defaults.thumbnailOrientation ?? 'portrait', thumbnailImageFit: defaults.thumbnailImageFit ?? 'cover' }
  }
  /** Toggles one pin after confirmation. @param name Profile-local key. */
  async function toggleSectionPinned(name: string): Promise<void> {
    if (name === BOOKMARKS_SECTION_PREFERENCE_KEY) return
    return run(async (context) => {
      const existing = sectionRecords.value.find((r) => r.name === name)
      if (existing) await deleteSection(name, existing.revision ?? 0, context)
      else await saveSection(draft(name), context)
    })
  }
  /** Moves a section within its visible subset using one complete atomic reorder.
   * @param name Profile-local key. @param direction Move direction. @param visibleKeys Optional catalogue-scoped keys.
   */
  async function movePinnedSection(name: string, direction: 'up' | 'down', visibleKeys?: string[]): Promise<void> {
    return run(async (context) => {
      if (!sectionRecords.value.some((r) => r.name === name)) { await saveSection(draft(name), context); sectionRecords.value = await getAllSections(context) }
      const all = [...sectionRecords.value].sort((a, b) => a.order - b.order)
      const visible = visibleKeys ? visibleKeys.filter((key) => all.some((r) => r.name === key)) : all.map((r) => r.name)
      const index = visible.indexOf(name), other = visible[index + (direction === 'up' ? -1 : 1)]
      if (index < 0 || !other) return
      const ids = all.map((r) => r.name), a = ids.indexOf(name), b = ids.indexOf(other)
      ;[ids[a], ids[b]] = [ids[b]!, ids[a]!]
      sectionRecords.value = await reorderSections(ids, context)
    })
  }
  /** Saves the selected display field, applying defaults only to missing records.
   * @param name Section key. @param field Changed display field. @param value Selected option. @param defaults New-record defaults.
   */
  function update(name: string, field: keyof SectionRecordDefaults, value: string, defaults: SectionRecordDefaults): Promise<void> {
    return run(async (context) => { await saveSection({ ...draft(name, defaults), [field]: value }, context) })
  }
  return {
    toggleSectionPinned, movePinnedSection,
    updateSectionCollectionMode: (name, mode, defaults = {}) => update(name, 'collectionMode', mode, defaults),
    updateSectionThumbnailOrientation: (name, orientation, defaults = {}) => update(name, 'thumbnailOrientation', orientation, defaults),
    updateSectionThumbnailImageFit: (name, fit, defaults = {}) => update(name, 'thumbnailImageFit', fit, defaults),
  }
}
