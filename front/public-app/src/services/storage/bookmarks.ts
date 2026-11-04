import type { Ref } from 'vue'

import {
  buildBookmarkKey,
  removeBookmark as removeBookmarkFromDB,
  setBookmark as setBookmarkInDB,
  type EntryBookmarkRecord,
} from '@/services/entryBookmarks'

import { sanitizeEntryBookmark } from './sanitizers'
import type { EntryBookmark } from './types'

/** Entry bookmark cache operations exposed by the storage store. */
export interface EntryBookmarksBridge {
  /** Stores or replaces the bookmark for one entry details page. */
  setEntryBookmark: (source: string, entry: string, bookmark: EntryBookmark) => void
  /** Removes the bookmark currently stored for one entry details page. */
  removeEntryBookmark: (source: string, entry: string) => void
}

/**
 * Creates the bookmark cache operations bound to the store's reactive record list.
 *
 * @param bookmarkRecords Reactive cache of entry bookmark records.
 * @returns Bookmark operations sharing the given cache.
 */
export function createEntryBookmarksBridge(
  bookmarkRecords: Ref<EntryBookmarkRecord[]>,
): EntryBookmarksBridge {
    /**
     * Stores or replaces the bookmark for one entry details page.
     *
     * Updates the local cache and persists to IndexedDB.
     *
     * @param source Backend source identifying the entry provider.
     * @param entry Absolute entry URL passed to `get_entry`.
     * @param bookmark Bookmark payload to persist.
     */
    function setEntryBookmark(source: string, entry: string, bookmark: EntryBookmark): void {
        const key = buildBookmarkKey(source, entry)
        const sanitized = sanitizeEntryBookmark(bookmark)
        const modifiedAt = Date.now()

        // Update cache
        const nextRecords = bookmarkRecords.value.filter((r) => r.key !== key)
        nextRecords.push({
          key,
          source: source.trim(),
          entry: entry.trim(),
          modifiedAt,
          ...sanitized,
        })
        bookmarkRecords.value = nextRecords

        // Persist to IndexedDB with the same modification timestamp as the cache
        void setBookmarkInDB(source, entry, sanitized, modifiedAt)
    }

    /**
     * Removes the bookmark currently stored for one entry details page.
     *
     * Updates the local cache and persists to IndexedDB.
     *
     * @param source Backend source identifying the entry provider.
     * @param entry Absolute entry URL passed to `get_entry`.
     */
    function removeEntryBookmark(source: string, entry: string): void {
        const key = buildBookmarkKey(source, entry)

        if (!bookmarkRecords.value.some((r) => r.key === key)) {
          return
        }

        // Update cache
        bookmarkRecords.value = bookmarkRecords.value.filter((r) => r.key !== key)

        // Persist to IndexedDB
        void removeBookmarkFromDB(source, entry)
    }

  return { setEntryBookmark, removeEntryBookmark }
}
