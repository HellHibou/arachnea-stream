import { getAllRecordsDB, getDB, initDB, loadRecordDB } from '@/services/indexDB'
import { validateCustomHomeDefinition } from '@/services/customHomeValidation'
import type { CustomHomeDefinition, CustomHomeListResult } from '@/types/customHome'
import { customHomeSlug } from '@/services/customHomeRoute'

const STORE_NAME = 'custom_homes'

export { validateCustomHomeDefinition } from '@/services/customHomeValidation'

/** @returns Additive store connection, preserving bookmarks and section preferences. */
export async function initCustomHomesDB(): Promise<IDBDatabase> {
  return initDB(STORE_NAME, 'id', ['navigationOrder'])
}

/**
 * Reads valid definitions and reports corrupt/unknown records without deleting them.
 * @returns Homes in navigation order and per-record read issues.
 */
export async function listCustomHomes(): Promise<CustomHomeListResult> {
  await initCustomHomesDB()
  const records = await getAllRecordsDB<Record<string, unknown>>(STORE_NAME)
  const result: CustomHomeListResult = { homes: [], issues: [] }
  for (const value of records) {
    try {
      result.homes.push(validateCustomHomeDefinition(value))
    } catch (error) {
      result.issues.push({
        id: typeof value?.id === 'string' ? value.id : null,
        message: error instanceof Error ? error.message : String(error),
      })
    }
  }
  result.homes.sort((left, right) => left.navigationOrder - right.navigationOrder || left.id.localeCompare(right.id))
  for (const home of result.homes) {
    const slug = customHomeSlug(home.name)
    if (!slug || result.homes.some((other) => other.id !== home.id && customHomeSlug(other.name) === slug)) {
      result.issues.push({ id: home.id, message: `Accueil URL is empty or duplicated; rename this accueil: ${home.name}` })
    }
  }
  return result
}

/** @param id Stable home id. @returns Validated definition, or undefined if absent. */
export async function loadCustomHome(id: string): Promise<CustomHomeDefinition | undefined> {
  await initCustomHomesDB()
  const value = await loadRecordDB<Record<string, unknown>>(STORE_NAME, id)
  return value === undefined ? undefined : validateCustomHomeDefinition(value)
}

/**
 * Creates a detached empty draft without initializing storage or contacting providers.
 * @param name Non-blank display name.
 * @param navigationOrder Position in the navigation (default zero).
 * @returns Validated draft with merge disabled and bookmarks hidden by default.
 */
export function createCustomHomeDraft(name: string, navigationOrder = 0): CustomHomeDefinition {
  const now = Date.now()
  return validateCustomHomeDefinition({
    schemaVersion: 1, id: crypto.randomUUID(), name, navigationOrder,
    bannerSources: [], categoryShortcuts: [], blocks: [], sectionMergeMode: 'none',
    showBookmarks: false, createdAt: now, updatedAt: now,
  })
}

/**
 * Commits a validated record with an atomic revision comparison in the same transaction.
 * @param definition Draft to save; no write occurs until the complete transaction commits.
 * @param expectedUpdatedAt Last read timestamp, or null for a new definition.
 * @returns Committed definition with a strictly increasing update timestamp.
 */
export async function saveCustomHome(
  definition: CustomHomeDefinition,
  expectedUpdatedAt: number | null,
): Promise<CustomHomeDefinition> {
  const draft = validateCustomHomeDefinition(definition)
  if (!customHomeSlug(draft.name)) throw new Error('Custom home name must contain letters or numbers')
  const committed = await mutateCustomHome(draft.id, expectedUpdatedAt, draft)
  if (!committed) throw new Error(`Custom home ${draft.id} was not saved`)
  return committed
}

/**
 * Deletes only the revision the caller read, avoiding silent removal of newer edits.
 * @param id Stable definition id.
 * @param expectedUpdatedAt Last read timestamp.
 * @returns Promise resolved only once deletion commits.
 */
export async function deleteCustomHome(id: string, expectedUpdatedAt: number): Promise<void> {
  await mutateCustomHome(id, expectedUpdatedAt)
}

/**
 * Commits the complete navigation order atomically, refusing concurrent additions or changes.
 * @param homes Ordered snapshots, each retaining its last-read revision.
 * @returns Promise resolved only after all order changes commit.
 */
export async function reorderCustomHomes(homes: CustomHomeDefinition[]): Promise<void> {
  const snapshots = homes.map(validateCustomHomeDefinition)
  if (new Set(snapshots.map((home) => home.id)).size !== snapshots.length) {
    throw new Error('Custom home order contains duplicate ids')
  }
  await initCustomHomesDB()
  const db = await getDB()
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite')
    const store = tx.objectStore(STORE_NAME)
    const request = store.getAll()
    let failure: unknown
    request.onsuccess = () => {
      try {
        const current = (request.result as unknown[]).map(validateCustomHomeDefinition)
        if (current.length !== snapshots.length || snapshots.some((snapshot) =>
          current.find((home) => home.id === snapshot.id)?.updatedAt !== snapshot.updatedAt)) {
          throw new Error('Custom homes changed in another view; reload before reordering')
        }
        snapshots.forEach((snapshot, navigationOrder) => {
          if (snapshot.navigationOrder !== navigationOrder) store.put({ ...snapshot, navigationOrder,
            updatedAt: Math.max(Date.now(), snapshot.updatedAt + 1) })
        })
      } catch (error) { failure = error; tx.abort() }
    }
    tx.oncomplete = () => resolve()
    tx.onabort = () => reject(failure ?? tx.error ?? new Error('Custom home order transaction aborted'))
    tx.onerror = () => reject(failure ?? tx.error ?? new Error('Custom home order transaction failed'))
  })
}

/**
 * Performs a compare-and-write/delete transaction, including across tabs and native views.
 * @param id Primary key.
 * @param expectedUpdatedAt Expected persisted revision; null requires a missing record.
 * @param draft Validated replacement; omitted deletes the record.
 * @returns Committed definition for writes; undefined for deletion.
 */
async function mutateCustomHome(
  id: string,
  expectedUpdatedAt: number | null,
  draft?: CustomHomeDefinition,
): Promise<CustomHomeDefinition | undefined> {
  if (!id.trim() || (expectedUpdatedAt !== null &&
    (!Number.isSafeInteger(expectedUpdatedAt) || expectedUpdatedAt < 0))) {
    throw new Error('Custom home mutation requires a valid id and expected revision')
  }
  await initCustomHomesDB()
  const db = await getDB()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite')
    const store = tx.objectStore(STORE_NAME)
    const request = store.getAll()
    let committed: CustomHomeDefinition | undefined
    let failure: unknown
    request.onsuccess = () => {
      try {
        const records = request.result as Array<Record<string, unknown>>
        const value = records.find((record) => record.id === id)
        const current = value === undefined ? undefined : validateCustomHomeDefinition(value)
        if ((current?.updatedAt ?? null) !== expectedUpdatedAt) {
          throw new Error(`Custom home ${id} changed in another view; reload before saving or deleting`)
        }
        if (draft) {
          if (records.some((record) => record.id !== id && typeof record.name === 'string' &&
            customHomeSlug(record.name) === customHomeSlug(draft.name))) {
            throw new Error(`An accueil with the same name or URL already exists: ${draft.name}`)
          }
          committed = validateCustomHomeDefinition({
            ...draft,
            createdAt: current?.createdAt ?? draft.createdAt,
            updatedAt: Math.max(Date.now(), draft.updatedAt, (current?.updatedAt ?? -1) + 1),
          })
          store.put(committed)
        } else {
          store.delete(id)
        }
      } catch (error) {
        failure = error
        tx.abort()
      }
    }
    tx.oncomplete = () => resolve(committed)
    tx.onabort = () => reject(failure ?? tx.error ?? new Error(`Custom home ${id} transaction aborted`))
    tx.onerror = () => reject(failure ?? tx.error ?? new Error(`Custom home ${id} transaction failed`))
  })
}