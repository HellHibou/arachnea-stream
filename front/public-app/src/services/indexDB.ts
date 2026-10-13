/** Name of the IndexedDB database. */
const DB_NAME = 'arachnea-db'

let dbPromise: Promise<IDBDatabase> | null = null
let initChain: Promise<void> = Promise.resolve()
const stores = new Map<string, { keyPath: string; indexFields: string[] }>()

/** @param db Open connection. Watches close/version changes so stale connections are never reused. */
function watchConnection(db: IDBDatabase): void {
  const invalidate = () => {
    if (dbPromise) {
      void dbPromise.catch(() => undefined).then((current) => {
        if (current === db) dbPromise = null
      })
    }
  }
  db.onclose = invalidate
  db.onversionchange = () => {
    db.close()
    invalidate()
  }
}

/**
 * Initializes (or updates) an object store and its indexes in the IndexedDB
 * database, then returns the database connection.
 *
 * The function is idempotent: it can be called multiple times to create multiple
 * stores. Concurrent calls are serialized so a second initialization never closes
 * the connection used by the first one. The database version increases only when
 * stores are missing.
 *
 * @param storeName - Name of the object store to create (e.g. 'home_preferences').
 * @param keyPath - Name of the field used as the primary key (e.g. 'name').
 * @param indexFields - Array of field names to index (e.g. ['order']).
 *                      An index is created for each field, with the same name as the field.
 * @returns A promise resolved with the {@link IDBDatabase} connection.
 * @throws {Error} If the database upgrade is blocked by another tab.
 */
export function initDB(
  storeName: string,
  keyPath: string,
  indexFields: string[] = [],
): Promise<IDBDatabase> {
  stores.set(storeName, { keyPath, indexFields })

  const run = initChain.then(async () => {
    if (dbPromise) {
      try {
        const currentDb = await dbPromise
        const missingStores = [...stores.keys()].filter(
          (name) => !currentDb.objectStoreNames.contains(name),
        )
        if (missingStores.length === 0) return currentDb
        // An upgrade is required: this connection must be closed first so the
        // version change is not blocked by this tab.
        currentDb.close()
        dbPromise = null
      } catch {
        dbPromise = null
        // A blocked upgrade must not prevent a later retry after the other tab closes.
      }
    }

    // Start from the database's existing version (if any), so a page reload
    // does not try to open a lower version than stored. A fresh browser has
    // no database yet: open version 1 (IndexedDB rejects version 0).
    let currentVersion: number | undefined
    if (typeof indexedDB.databases === 'function') {
      const existingDatabases = await indexedDB.databases()
      const existing = existingDatabases.find((db) => db.name === DB_NAME)
      currentVersion = existing?.version ?? 1
    } else {
      currentVersion = undefined
    }

    const currentDb = await openDB(currentVersion)
    const missingStores = [...stores.keys()].filter(
      (name) => !currentDb.objectStoreNames.contains(name),
    )

    if (missingStores.length === 0) {
      dbPromise = Promise.resolve(currentDb)
      return currentDb
    }

    // Some stores are missing: close and reopen at the next version to create them.
    const nextVersion = currentDb.version + 1
    currentDb.close()
    const upgradedDb = await openDB(nextVersion)
    dbPromise = Promise.resolve(upgradedDb)
    return upgradedDb
  })
  initChain = run.then(
    () => undefined,
    () => undefined,
  )

  return run
}

/**
 * Opens the IndexedDB database at the given version, creating any missing
 * stores declared in the `stores` map during the upgrade.
 *
 * @param version - Database version to open; omitted opens the current version.
 * @returns A promise resolved with the opened connection.
 * @throws {Error} If the database upgrade is blocked by another tab.
 */
function openDB(version?: number): Promise<IDBDatabase> {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, version)
    let blocked = false

    request.onupgradeneeded = () => {
      if (blocked) {
        request.transaction?.abort()
        return
      }
      const db = request.result
      for (const [name, def] of stores) {
        if (db.objectStoreNames.contains(name)) continue
        const store = db.createObjectStore(name, { keyPath: def.keyPath })
        for (const field of def.indexFields) {
          store.createIndex(field, field)
        }
      }
    }

    request.onsuccess = () => {
      const db = request.result
      watchConnection(db)
      if (blocked) db.close()
      else resolve(db)
    }
    request.onerror = () => reject(request.error)
    request.onblocked = () => {
      blocked = true
      reject(new Error('Database upgrade blocked by another tab'))
    }
  })
}

/**
 * Returns the connection to the previously opened IndexedDB database.
 *
 * When the cached connection was invalidated by a close or version change,
 * known stores are reopened automatically so callers never transact on a
 * closed database.
 *
 * @internal
 * @returns The cached {@link IDBDatabase} connection.
 * @throws {Error} If {@link initDB} has not been called yet.
 */
export async function getDB(): Promise<IDBDatabase> {
  if (dbPromise) {
    try {
      return await dbPromise
    } catch {
      dbPromise = null
    }
  }
  if (stores.size === 0) {
    throw new Error("initDB() must be called before any other operation")
  }
  let reopened: IDBDatabase | undefined
  for (const [name, config] of stores) {
    reopened = await initDB(name, config.keyPath, config.indexFields)
  }
  if (!reopened) throw new Error("initDB() must be called before any other operation")
  return reopened
}

/** @param error Transaction failure. @returns Whether the connection was closed and a reopen may help. */
function isClosedDatabaseError(error: unknown): boolean {
  if (error instanceof DOMException) {
    return error.name === 'InvalidStateError' || /closed/i.test(error.message)
  }
  return error instanceof Error && /closed database/i.test(error.message)
}

/**
 * Runs an IndexedDB operation, reopening known stores once when the cached
 * connection was closed (multi-tab upgrade, concurrent initialization).
 * @typeParam T Operation result.
 * @param operation Task using a live connection.
 * @returns Operation result.
 */
export async function withDatabaseRetry<T>(operation: (db: IDBDatabase) => Promise<T>): Promise<T> {
  try {
    return await operation(await getDB())
  } catch (error) {
    if (!isClosedDatabaseError(error)) throw error
    dbPromise = null
    return operation(await getDB())
  }
}

/**
 * Reads all records from a store, sorted by a given field.
 *
 * If `sortField` is provided and an index with that name exists on the store,
 * records are read through the index cursor (ascending order).
 * Otherwise (missing, empty or non-indexed field), records are read through the
 * store cursor, i.e. sorted by primary key (the "id").
 *
 * @typeParam T - Type of the returned records (must be an object).
 * @param storeName - Name of the object store to read.
 * @param sortField - Field to sort by. If missing or not indexed, sort by primary key.
 * @returns A promise resolved with the sorted records array.
 * @throws {Error} If {@link initDB} has not been called for this store.
 */
export async function getAllRecordsDB<T extends Record<string, any>>(
  storeName: string,
  sortField?: string,
): Promise<T[]> {
  return withDatabaseRetry((db) => new Promise<T[]>((resolve, reject) => {
    let tx: IDBTransaction
    try {
      tx = db.transaction(storeName, 'readonly')
    } catch (error) { reject(error); return }
    const store = tx.objectStore(storeName)

    const useIndex = !!sortField && store.indexNames.contains(sortField)
    const request = useIndex
      ? store.index(sortField!).openCursor()
      : store.openCursor()

    const results: T[] = []
    request.onsuccess = () => {
      const cursor = request.result
      if (cursor) {
        results.push(cursor.value)
        cursor.continue()
      } else {
        resolve(results)
      }
    }
    request.onerror = () => reject(request.error)
  }))
}

/**
 * Loads a single record from a store, identified by its primary key.
 *
 * @typeParam T - Type of the returned record (must be an object).
 * @param storeName - Name of the object store to query.
 * @param key - Value of the primary key of the record to find (e.g. the "id").
 * @returns A promise resolved with the found record, or `undefined` if it does not exist.
 * @throws {Error} If {@link initDB} has not been called for this store, or if the read fails.
 */
export async function loadRecordDB<T extends Record<string, any>>(
  storeName: string,
  key: IDBValidKey,
): Promise<T | undefined> {
  return withDatabaseRetry((db) => new Promise<T | undefined>((resolve, reject) => {
    let tx: IDBTransaction
    try {
      tx = db.transaction(storeName, 'readonly')
    } catch (error) { reject(error); return }
    const request = tx.objectStore(storeName).get(key)
    request.onsuccess = () => resolve(request.result as T | undefined)
    request.onerror = () => reject(request.error)
  }))
}

/**
 * Saves a record into the given store (insert or update).
 *
 * Uses `put`: if the primary key already exists, the record is replaced,
 * otherwise it is added.
 *
 * @typeParam T - Type of the record to save.
 * @param storeName - Name of the destination object store.
 * @param record - Record to save, must contain the primary key field.
 * @returns A promise resolved once the write has completed.
 * @throws {Error} If {@link initDB} has not been called for this store, or if the write fails.
 */
export async function saveRecordDB<T extends Record<string, any>>(
  storeName: string,
  record: T,
): Promise<void> {
  return withDatabaseRetry((db) => new Promise<void>((resolve, reject) => {
    let tx: IDBTransaction
    try {
      tx = db.transaction(storeName, 'readwrite')
    } catch (error) { reject(error); return }
    tx.objectStore(storeName).put(record)
    tx.oncomplete = () => resolve()
    tx.onerror = () => reject(tx.error)
  }))
}

/**
 * Deletes a record from the given store, identified by its primary key.
 *
 * @param storeName - Name of the object store to delete from.
 * @param key - Value of the primary key of the record to delete.
 * @returns A promise resolved once the deletion has completed.
 * @throws {Error} If {@link initDB} has not been called for this store, or if the deletion fails.
 */
export async function deleteRecordDB(
  storeName: string,
  key: IDBValidKey,
): Promise<void> {
  return withDatabaseRetry((db) => new Promise<void>((resolve, reject) => {
    let tx: IDBTransaction
    try {
      tx = db.transaction(storeName, 'readwrite')
    } catch (error) { reject(error); return }
    tx.objectStore(storeName).delete(key)
    tx.oncomplete = () => resolve()
    tx.onerror = () => reject(tx.error)
  }))
}
