import { encodeEntryRoutePayload } from '@/router/routePayloads'
import { initDB, getAllRecordsDB, loadRecordDB, saveRecordDB, deleteRecordDB } from '@/services/indexDB'
import type { EntryBookmark } from '@/services/storage'

/** Name of the IndexedDB object store for entry bookmarks. */
const STORE_NAME = 'entry_bookmarks'
/** Primary key field name. */
const KEY_PATH = 'key'

/**
 * Reserved preference key of the local bookmarks home section.
 *
 * The `local:` prefix guarantees no collision with backend-provided section keys.
 */
export const BOOKMARKS_SECTION_PREFERENCE_KEY = 'local:bookmarks'

/**
 * Record persisted for one entry bookmark.
 * Extends EntryBookmark with a composite primary key derived from source and entry.
 */
export type EntryBookmarkRecord = EntryBookmark & {
  /** Primary key, the base64url route token derived from source + entry. */
  key: string
  /** Backend source identifying the entry provider. */
  source: string
  /** Absolute entry URL passed to `get_entry`. */
  entry: string
  /** Timestamp (ms epoch) of the last record modification. */
  modifiedAt: number
}

/**
 * Initializes the `entry_bookmarks` object store if it has not been created yet.
 *
 * @returns A promise resolved with the IDBDatabase connection.
 */
export async function initEntryBookmarksDB(): Promise<IDBDatabase> {
  return initDB(STORE_NAME, KEY_PATH)
}

/**
 * Builds the storage key used for one entry bookmark.
 *
 * The key is the base64url route token for the entry details page, identical
 * to the `encodedEntry` segment of the `/entry/:encodedEntry` route.
 *
 * @param source - Backend source identifying the entry provider.
 * @param entry - Absolute entry URL passed to `get_entry`.
 * @returns Stable bookmark key used in IndexedDB.
 */
export function buildBookmarkKey(source: string, entry: string): string {
  return encodeEntryRoutePayload({
    source: source.trim(),
    entryUrl: entry.trim(),
  })
}

/**
 * Returns all entry bookmark records.
 *
 * @returns A promise resolved with the array of bookmark records.
 */
export async function getAllBookmarks(): Promise<EntryBookmarkRecord[]> {
  return getAllRecordsDB<EntryBookmarkRecord>(STORE_NAME)
}

/**
 * Loads the bookmark for one entry details page.
 *
 * @param source - Backend source identifying the entry provider.
 * @param entry - Absolute entry URL passed to `get_entry`.
 * @returns A promise resolved with the bookmark, or `null` when none exists.
 */
export async function getBookmark(
  source: string,
  entry: string,
): Promise<EntryBookmark | null> {
  const key = buildBookmarkKey(source, entry)
  const record = await loadRecordDB<EntryBookmarkRecord>(STORE_NAME, key)
  return record ?? null
}

/**
 * Stores or replaces the bookmark for one entry details page.
 *
 * @param source - Backend source identifying the entry provider.
 * @param entry - Absolute entry URL passed to `get_entry`.
 * @param bookmark - Bookmark payload to persist.
 * @param modifiedAt - Timestamp (ms epoch) of the modification, shared with the reactive cache.
 * @returns A promise resolved once the write has completed.
 */
export async function setBookmark(
  source: string,
  entry: string,
  bookmark: EntryBookmark,
  modifiedAt: number,
): Promise<void> {
  const key = buildBookmarkKey(source, entry)
  const record: EntryBookmarkRecord = {
    key,
    source: source.trim(),
    entry: entry.trim(),
    modifiedAt,
    ...bookmark,
  }
  return saveRecordDB(STORE_NAME, record)
}

/**
 * Removes the bookmark for one entry details page.
 *
 * @param source - Backend source identifying the entry provider.
 * @param entry - Absolute entry URL passed to `get_entry`.
 * @returns A promise resolved once the deletion has completed.
 */
export async function removeBookmark(
  source: string,
  entry: string,
): Promise<void> {
  const key = buildBookmarkKey(source, entry)
  return deleteRecordDB(STORE_NAME, key)
}