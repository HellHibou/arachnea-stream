/** Name of the IndexedDB database. */
const DB_NAME = 'arachnea-db'

let dbPromise: Promise<IDBDatabase> | null = null
const stores = new Map<string, { keyPath: string; indexFields: string[] }>()

/**
 * Initializes (or updates) an object store and its indexes in the IndexedDB
 * database, then returns the database connection.
 *
 * The function is idempotent: it can be called multiple times to create multiple
 * stores. Each call increments the database version, which triggers
 * `onupgradeneeded` and allows the creation of missing stores.
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

  const previousPromise = dbPromise
  dbPromise = (async () => {
    // Close the previous connection before opening a new one, so any
    // sequential upgrade performed in this tab is not blocked.
    if (previousPromise) {
      const previousDb = await previousPromise
      previousDb.close()
    }

    // Start from the database's existing version (if any), so a page reload
    // does not try to open a lower version than stored.
    let currentVersion: number
    if (typeof indexedDB.databases === 'function') {
      const existingDatabases = await indexedDB.databases()
      const existing = existingDatabases.find((db) => db.name === DB_NAME)
      currentVersion = existing?.version ?? 0
    } else {
      currentVersion = 0
    }

    const currentDb = await openDB(currentVersion)
    const missingStores = [...stores.keys()].filter(
      (name) => !currentDb.objectStoreNames.contains(name),
    )

    if (missingStores.length === 0) {
      return currentDb
    }

    // Some stores are missing: close and reopen at the next version to create them.
    currentDb.close()
    return openDB(currentVersion + 1)
  })()

  return dbPromise
}

/**
 * Opens the IndexedDB database at the given version, creating any missing
 * stores declared in the `stores` map during the upgrade.
 *
 * @param version - Database version to open.
 * @returns A promise resolved with the opened connection.
 * @throws {Error} If the database upgrade is blocked by another tab.
 */
function openDB(version: number): Promise<IDBDatabase> {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, version)

    request.onupgradeneeded = () => {
      const db = request.result
      for (const [name, def] of stores) {
        if (db.objectStoreNames.contains(name)) continue
        const store = db.createObjectStore(name, { keyPath: def.keyPath })
        for (const field of def.indexFields) {
          store.createIndex(field, field)
        }
      }
    }

    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
    request.onblocked = () =>
      reject(new Error('Database upgrade blocked by another tab'))
  })
}

/**
 * Returns the connection to the previously opened IndexedDB database.
 *
 * @internal
 * @returns The cached {@link IDBDatabase} connection.
 * @throws {Error} If {@link initDB} has not been called yet.
 */
async function getDB(): Promise<IDBDatabase> {
  if (!dbPromise) {
    throw new Error("initDB() must be called before any other operation")
  }
  return dbPromise
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
  const db = await getDB()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(storeName, 'readonly')
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
  })
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
  const db = await getDB()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(storeName, 'readonly')
    const request = tx.objectStore(storeName).get(key)
    request.onsuccess = () => resolve(request.result as T | undefined)
    request.onerror = () => reject(request.error)
  })
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
  const db = await getDB()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(storeName, 'readwrite')
    tx.objectStore(storeName).put(record)
    tx.oncomplete = () => resolve()
    tx.onerror = () => reject(tx.error)
  })
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
  const db = await getDB()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(storeName, 'readwrite')
    tx.objectStore(storeName).delete(key)
    tx.oncomplete = () => resolve()
    tx.onerror = () => reject(tx.error)
  })
}
