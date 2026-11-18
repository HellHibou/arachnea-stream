import { createUuid } from '@/utils'
import { validateCustomHomeDefinition } from '@/services/customHomeValidation'
import type { CustomHomeDefinition, CustomHomeListResult } from '@/types/customHome'
import { profileCall, profileWrite, hasProfile, type ProfileCollection } from '@/services/profileApi'
import { requireProfileContext } from '@/services/profileContext'
import { loadDefaultHome } from '@/services/profileDefaultHome'

export { validateCustomHomeDefinition } from '@/services/customHomeValidation'
interface HomeRow { home_id: string; revision: number; definition: CustomHomeDefinition }
let collection: ProfileCollection<HomeRow> = { items: [], revision: 1 }
const versions = new Map<string, number>()
/** Remembers confirmed revisions separately from immutable editor snapshots. @param row Server home record. */
function remember(row: HomeRow): CustomHomeDefinition {
  const home = validateCustomHomeDefinition(row.definition)
  versions.set(`${home.id}:${home.updatedAt}`, row.revision)
  return home
}
/** @returns Server homes in navigation order; no browser data is imported. */
export async function listCustomHomes(): Promise<CustomHomeListResult> {
  if (!hasProfile()) return { homes: [], issues: [] }
  collection = await profileCall<ProfileCollection<HomeRow>>('homes')
  return { homes: collection.items.map(remember), issues: [] }
}
/** @param id Common-profile-local home UUID. @returns Confirmed home or undefined. */
export async function loadCustomHome(id: string): Promise<CustomHomeDefinition | undefined> {
  return (await listCustomHomes()).homes.find((home) => home.id === id)
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
    schemaVersion: 1, id: createUuid(), name, navigationOrder,
    bannerSources: [], categoryShortcuts: [], blocks: [], sectionMergeMode: 'none',
    showBookmarks: false, createdAt: now, updatedAt: now,
  })
}


/**
 * Saves a complete home using the revision corresponding to the caller's snapshot.
 * @param definition Detached draft; a new draft receives its authoritative server UUID.
 * @param expectedUpdatedAt Last-read timestamp, or null for create-only.
 * @returns Confirmed server definition; failures leave the draft untouched.
 */
export async function saveCustomHome(definition: CustomHomeDefinition, expectedUpdatedAt: number | null): Promise<CustomHomeDefinition> {
  const draft = validateCustomHomeDefinition(definition)
  const revision = expectedUpdatedAt === null ? 0 : versions.get(`${draft.id}:${expectedUpdatedAt}`)
  if (revision === undefined) throw new Error('Reload this home before saving its changes.')
  const value = expectedUpdatedAt === null
    ? { definition: Object.fromEntries(Object.entries(draft).filter(([key]) => key !== 'id')) }
    : { home_id: draft.id, definition: draft }
  const row = await profileWrite<HomeRow>('save_home', { expected_revision: revision, value }, `home:${draft.id}`)
  return remember(row)
}
/** Deletes the last-read home revision and refreshes the server-owned default reference.
 * @param id Stable home UUID. @param expectedUpdatedAt Timestamp of the confirmed snapshot.
 */
export async function deleteCustomHome(id: string, expectedUpdatedAt: number): Promise<void> {
  const revision = versions.get(`${id}:${expectedUpdatedAt}`)
  if (revision === undefined) throw new Error('Reload this home before deleting it.')
  const context = requireProfileContext()
  await profileWrite('delete_home', { expected_revision: revision, identity: { home_id: id } }, `home:${id}`, context)
  await loadDefaultHome(context)
}
/** Atomically replaces the complete navigation order, rejecting stale snapshots.
 * @param homes Ordered confirmed home snapshots.
 */
export async function reorderCustomHomes(homes: CustomHomeDefinition[]): Promise<void> {
  if (homes.length !== collection.items.length || homes.some((home) => !collection.items.some((row) => row.home_id === home.id && row.definition.updatedAt === home.updatedAt))) {
    throw new Error('Homes changed; reload before reordering.')
  }
  collection = await profileWrite<ProfileCollection<HomeRow>>('reorder_homes', { expected_revision: collection.revision, ids: homes.map((home) => home.id) }, 'homes:order')
  collection.items.forEach(remember)
}
window.addEventListener('arachnea-profile-context', () => { collection = { items: [], revision: 1 }; versions.clear() })
