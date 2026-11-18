import { encodeEntryRoutePayload } from '@/router/routePayloads'
import type { EntryBookmark } from '@/services/storage/types'
import { hasProfile, profileCall, profileWrite, type ProfileCollection } from '@/services/profileApi'
import { requireProfileContext, type ProfileContext } from '@/services/profileContext'

/** Reserved profile-local bookmarks section key, independent from provider keys. */
export const BOOKMARKS_SECTION_PREFERENCE_KEY = 'local:bookmarks'
/** Confirmed bookmark with frontend route identity and server metadata. */
export type EntryBookmarkRecord = EntryBookmark & { key: string; source: string; entry: string; modifiedAt: number; revision: number }
interface BookmarkRow { source_id: string; entry: string; modified_at: number; revision: number; bookmark: EntryBookmark }
/** @param source Provider identity. @param entry Exact trimmed entry URL. @returns Display/route key, never authorization. */
export function buildBookmarkKey(source: string, entry: string): string { return encodeEntryRoutePayload({ source: source.trim(), entryUrl: entry.trim() }) }
/** @param row Server-owned identity and timestamp. @returns Existing frontend record shape. */
function record(row: BookmarkRow): EntryBookmarkRecord { return { ...row.bookmark, key: buildBookmarkKey(row.source_id, row.entry), source: row.source_id, entry: row.entry, modifiedAt: row.modified_at, revision: row.revision } }
/** @returns Confirmed bookmarks for the selected profile, without reading IndexedDB. */
export async function getAllBookmarks(): Promise<EntryBookmarkRecord[]> {
  if (!hasProfile()) return []
  const collection = await profileCall<ProfileCollection<BookmarkRow>>('bookmarks')
  return collection.items.map(record)
}
/** @param source Provider identity. @param entry Exact entry URL. @returns Confirmed bookmark or null. */
export async function getBookmark(source: string, entry: string): Promise<EntryBookmark | null> { const key = buildBookmarkKey(source, entry); return (await getAllBookmarks()).find((r) => r.key === key) ?? null }
/** Saves a full replacement after the prior revision is confirmed.
 * @param source Provider identity. @param entry Entry URL. @param bookmark Complete snapshot.
 * @param expected_revision Confirmed record revision, or zero for create-only. @param context Captured profile.
 */
export async function setBookmark(source: string, entry: string, bookmark: EntryBookmark, expected_revision: number, context: ProfileContext = requireProfileContext()): Promise<EntryBookmarkRecord> {
  const row = await profileWrite<BookmarkRow>('save_bookmark', { expected_revision, value: { source_id: source.trim(), entry: entry.trim(), bookmark } }, `bookmark:${buildBookmarkKey(source, entry)}`, context)
  return record(row)
}
/** Deletes a confirmed bookmark version.
 * @param source Provider identity. @param entry Entry URL. @param expected_revision Confirmed version. @param context Captured profile.
 */
export async function removeBookmark(source: string, entry: string, expected_revision: number, context: ProfileContext = requireProfileContext()): Promise<void> {
  await profileWrite('delete_bookmark', { expected_revision, identity: { source_id: source.trim(), entry: entry.trim() } }, `bookmark:${buildBookmarkKey(source, entry)}`, context)
}
