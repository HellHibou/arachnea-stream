import type { MediaCardCollectionMode, ThumbnailImageFit, ThumbnailOrientation } from '@/types/media'
import { requireProfileContext, type ProfileContext } from '@/services/profileContext'
import { hasProfile, profileCall, profileWrite, type ProfileCollection } from '@/services/profileApi'

/** Confirmed profile-local pinned section and display preferences. */
export type HomeSectionRecord = {
  name: string
  order: number
  collectionMode: MediaCardCollectionMode
  thumbnailOrientation: ThumbnailOrientation
  thumbnailImageFit: ThumbnailImageFit
  revision?: number
}
interface SectionRow extends Omit<HomeSectionRecord, 'name'> { section_key: string; revision: number }
let revision = 1
/** @param row Server section metadata. @returns Existing frontend shape plus confirmed revision. */
function record(row: SectionRow): HomeSectionRecord { return { name: row.section_key, order: row.order, revision: row.revision, collectionMode: row.collectionMode, thumbnailOrientation: row.thumbnailOrientation, thumbnailImageFit: row.thumbnailImageFit } }
/** @returns Profile sections from the server, sorted by order. */
export async function getAllSections(context?: ProfileContext): Promise<HomeSectionRecord[]> {
  if (!hasProfile()) return []
  const collection = await profileCall<ProfileCollection<SectionRow>>('sections', {}, false, context ?? requireProfileContext())
  revision = collection.revision
  return collection.items.map(record)
}
/** @param name Profile-local preference key. @returns Confirmed pinned section, if present. */
export async function getSection(name: string): Promise<HomeSectionRecord | undefined> { return (await getAllSections()).find((r) => r.name === name) }
/** Saves a complete section replacement using its last-read revision. @param section Existing record or create-only draft. */
export async function saveSection(section: HomeSectionRecord, context?: ProfileContext): Promise<HomeSectionRecord> {
  const { name, revision: expected_revision = 0, ...values } = section
  const row = await profileWrite<SectionRow>('save_section', { expected_revision, value: { section_key: name, ...values } }, `section:${name}`, context ?? requireProfileContext())
  return record(row)
}
/** Deletes only the confirmed section version. @param name Preference key. @param expected_revision Last-read version. */
export async function deleteSection(name: string, expected_revision: number, context?: ProfileContext): Promise<void> {
  await profileWrite('delete_section', { expected_revision, identity: { section_key: name } }, `section:${name}`, context ?? requireProfileContext())
}
/** Replaces the complete section order atomically; unknown/missing IDs are rejected. @param orderedNames Full ordered preference-key list. */
export async function reorderSections(orderedNames: string[], context?: ProfileContext): Promise<HomeSectionRecord[]> {
  const collection = await profileWrite<ProfileCollection<SectionRow>>('reorder_sections', { expected_revision: revision, ids: orderedNames }, 'sections:order', context ?? requireProfileContext())
  revision = collection.revision
  return collection.items.map(record)
}
window.addEventListener('arachnea-profile-context', () => { revision = 1 })
