import { shallowRef, type Ref } from 'vue'
import { buildBookmarkKey, removeBookmark, setBookmark, type EntryBookmarkRecord } from '@/services/entryBookmarks'
import { isCurrentProfile, registerProfileFlusher, requireProfileContext, type ProfileContext } from '@/services/profileContext'
import { reportPersonalDataFailure } from '@/services/personalDataStatus'
import { sanitizeEntryBookmark } from './sanitizers'
import type { EntryBookmark } from './types'

/** Maximum interval between ordinary playback-progress writes, in milliseconds. */
const PLAYBACK_WRITE_INTERVAL_MS = 10_000
/** Best-effort profile switching must not wait indefinitely for a disconnected server. */
const PROFILE_FLUSH_TIMEOUT_MS = 5_000
/** Number of bookmark identities with unconfirmed in-memory changes. */
export const pendingBookmarkWrites = shallowRef(0)
interface Change { bookmark: EntryBookmark | null; immediate: boolean }
interface Queue {
  context: ProfileContext; source: string; entry: string; revision: number; lastSent: number
  pending?: Change; retry?: Change; flight?: Promise<void>; timer?: ReturnType<typeof setTimeout>
}
/** Bookmark cache and explicit retry operations. */
export interface EntryBookmarksBridge {
  setEntryBookmark: (source: string, entry: string, bookmark: EntryBookmark, immediate?: boolean) => void
  removeEntryBookmark: (source: string, entry: string) => void
  flushEntryBookmarks: () => Promise<void>
  reconcileBookmarkRecords: (records: EntryBookmarkRecord[]) => EntryBookmarkRecord[]
}

/** Serializes each bookmark, collapses unsent progress and retains failed writes only in memory.
 * @param bookmarkRecords Reactive selected-profile cache; no browser persistence is used.
 */
export function createEntryBookmarksBridge(bookmarkRecords: Ref<EntryBookmarkRecord[]>): EntryBookmarksBridge {
  const queues = new Map<string, Queue>()
  /** Updates the visible count from pending, failed and in-flight identities. */
  function count(): void { pendingBookmarkWrites.value = [...queues.values()].filter((q) => q.pending || q.retry || q.flight).length }
  /** Updates an optimistic snapshot while retaining its confirmed revision. @param key Route key. @param q Captured queue. @param change Latest desired snapshot. */
  function optimistic(key: string, q: Queue, change: Change): void {
    bookmarkRecords.value = bookmarkRecords.value.filter((r) => r.key !== key)
    if (change.bookmark) bookmarkRecords.value.push({ ...change.bookmark, key, source: q.source, entry: q.entry, revision: q.revision, modifiedAt: Date.now() })
  }
  /** Schedules only unsent progress; failed requests require an explicit retry/flush. @param key Route key. @param q Captured queue. */
  function schedule(key: string, q: Queue): void {
    if (q.flight || q.retry || !q.pending || !isCurrentProfile(q.context)) return
    if (q.timer) clearTimeout(q.timer)
    const delay = q.pending.immediate ? 0 : Math.max(0, q.lastSent + PLAYBACK_WRITE_INTERVAL_MS - Date.now())
    if (delay === 0) void send(key, q)
    else q.timer = setTimeout(() => { q.timer = undefined; void send(key, q) }, delay)
  }
  /** Sends one immutable request and confirms its revision before sending the next.
   * @param key Route key. @param q Captured queue. @returns Resolves after this request, including a reported failure.
   */
  async function send(key: string, q: Queue): Promise<void> {
    if (q.flight) return q.flight
    if (!isCurrentProfile(q.context)) return
    const change = q.retry ?? q.pending
    if (!change) return
    if (q.timer) { clearTimeout(q.timer); q.timer = undefined }
    if (!q.retry) q.pending = undefined
    q.retry = undefined
    q.lastSent = Date.now()
    const work = async () => {
      try {
        const record = change.bookmark
          ? await setBookmark(q.source, q.entry, change.bookmark, q.revision, q.context)
          : (q.revision === 0 ? null : await removeBookmark(q.source, q.entry, q.revision, q.context), null)
        if (!isCurrentProfile(q.context)) return
        q.revision = record?.revision ?? 0
        if (q.pending) optimistic(key, q, q.pending)
        else {
          bookmarkRecords.value = bookmarkRecords.value.filter((r) => r.key !== key)
          if (record) bookmarkRecords.value.push(record)
        }
      } catch (error) {
        if (isCurrentProfile(q.context)) { q.retry = change; reportPersonalDataFailure(error) }
      } finally {
        q.flight = undefined
        count()
        schedule(key, q)
      }
    }
    q.flight = Promise.resolve().then(work)
    count()
    return q.flight
  }
  /** Captures a profile and confirmed version before enqueueing any asynchronous work.
   * @param source Provider. @param entry Entry URL. @param bookmark Full replacement, or null for deletion. @param immediate Whether to bypass playback throttling.
   */
  function enqueue(source: string, entry: string, bookmark: EntryBookmark | null, immediate: boolean): void {
    try {
      const context = requireProfileContext(), key = buildBookmarkKey(source, entry)
      let q = queues.get(key)
      if (!q || q.context.generation !== context.generation) {
        q = { context, source: source.trim(), entry: entry.trim(), revision: bookmarkRecords.value.find((r) => r.key === key)?.revision ?? 0, lastSent: 0 }
        queues.set(key, q)
      }
      q.pending = { bookmark: bookmark ? sanitizeEntryBookmark(bookmark) : null, immediate: immediate || q.pending?.immediate === true }
      optimistic(key, q, q.pending)
      schedule(key, q)
      count()
    } catch (error) { reportPersonalDataFailure(error) }
  }
  /** Retries failed immutable requests first, then sends the latest queued progress. */
  async function flushEntryBookmarks(): Promise<void> {
    await Promise.all([...queues].map(async ([key, q]) => {
      if (q.timer) { clearTimeout(q.timer); q.timer = undefined }
      for (let attempt = 0; attempt < 3; attempt += 1) {
        if (q.flight) await q.flight
        if (!q.pending && !q.retry) break
        await send(key, q)
        if (q.retry) break
      }
      if (q.flight) await q.flight
    }))
  }
  /** Preserves unconfirmed local snapshots during server refresh without rebasing their expected versions. @param records Fresh server records. */
  function reconcileBookmarkRecords(records: EntryBookmarkRecord[]): EntryBookmarkRecord[] {
    const pending = new Set([...queues].filter(([, q]) => q.pending || q.retry || q.flight).map(([key]) => key))
    for (const [key, q] of queues) if (!pending.has(key)) q.revision = records.find((r) => r.key === key)?.revision ?? 0
    return [...records.filter((r) => !pending.has(r.key)), ...bookmarkRecords.value.filter((r) => pending.has(r.key))]
  }
  registerProfileFlusher(async () => {
    let timer: ReturnType<typeof setTimeout> | undefined
    await Promise.race([flushEntryBookmarks(), new Promise<void>((resolve) => { timer = setTimeout(resolve, PROFILE_FLUSH_TIMEOUT_MS) })])
    if (timer) clearTimeout(timer)
  })
  window.addEventListener('arachnea-profile-context', () => { queues.forEach((q) => { if (q.timer) clearTimeout(q.timer) }); queues.clear(); count() })
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden') void flushEntryBookmarks() })
  return {
    setEntryBookmark: (source, entry, bookmark, immediate = true) => enqueue(source, entry, bookmark, immediate),
    removeEntryBookmark: (source, entry) => enqueue(source, entry, null, true),
    flushEntryBookmarks, reconcileBookmarkRecords,
  }
}
