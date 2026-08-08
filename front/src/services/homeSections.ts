import { initDB, getAllRecordsDB, loadRecordDB, saveRecordDB, deleteRecordDB } from '@/services/indexDB'
import type { MediaCardCollectionMode, ThumbnailImageFit, ThumbnailOrientation } from '@/types/media'

/** Name of the IndexedDB object store for home section preferences. */
const STORE_NAME = 'home_sections'
/** Primary key field name. */
const KEY_PATH = 'name'
/** Indexed fields for sorting. */
const INDEX_FIELDS = ['order']

/**
 * Record persisted for one pinned home section.
 */
export type HomeSectionRecord = {
  /** Primary key (mapped from HomeSection.preferenceKey). */
  name: string
  /** Display order of the section. */
  order: number
  /** Collection mode for the section. */
  collectionMode: MediaCardCollectionMode
  /** Thumbnail orientation for the section. */
  thumbnailOrientation: ThumbnailOrientation
  /** Thumbnail image fit for the section. */
  thumbnailImageFit: ThumbnailImageFit
}

/**
 * Initializes the `home_sections` object store if it has not been created yet.
 *
 * @returns A promise resolved with the IDBDatabase connection.
 */
export async function initHomeSectionsDB(): Promise<IDBDatabase> {
  return initDB(STORE_NAME, KEY_PATH, INDEX_FIELDS)
}

/**
 * Returns all pinned section records, sorted by their `order` field.
 *
 * @returns A promise resolved with the array of section records.
 */
export async function getAllSections(): Promise<HomeSectionRecord[]> {
  return getAllRecordsDB<HomeSectionRecord>(STORE_NAME, 'order')
}

/**
 * Loads a single pinned section record by its preference key.
 *
 * @param name - The section preference key (name).
 * @returns A promise resolved with the section record, or `undefined` if not found.
 */
export async function getSection(name: string): Promise<HomeSectionRecord | undefined> {
  return loadRecordDB<HomeSectionRecord>(STORE_NAME, name)
}

/**
 * Saves (inserts or updates) a pinned section record.
 *
 * @param record - The section record to persist.
 * @returns A promise resolved once the write has completed.
 */
export async function saveSection(record: HomeSectionRecord): Promise<void> {
  return saveRecordDB(STORE_NAME, record)
}

/**
 * Deletes a pinned section record by its preference key.
 *
 * @param name - The section preference key to remove.
 * @returns A promise resolved once the deletion has completed.
 */
export async function deleteSection(name: string): Promise<void> {
  return deleteRecordDB(STORE_NAME, name)
}

/**
 * Reorders pinned sections by rewriting the `order` field of each provided name.
 *
 * Sections not in the list are left untouched. The first entry gets `order = 0`,
 * the second `order = 1`, etc.
 *
 * @param orderedNames - Array of section preference keys in the desired order.
 * @returns A promise resolved once all order updates have completed.
 */
export async function reorderSections(orderedNames: string[]): Promise<void> {
  const updates = orderedNames.map((name, index) =>
    saveRecordDB(STORE_NAME, {
      name,
      order: index,
    } as HomeSectionRecord),
  )
  await Promise.all(updates)
}