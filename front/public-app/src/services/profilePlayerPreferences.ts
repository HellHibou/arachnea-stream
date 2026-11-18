import { readonly, ref } from 'vue'
import { profileCall, ProfileApiError, type OperationView, type ProfileSnapshot } from '@/services/profileApi'
import { isCurrentProfile, registerProfileFlusher, type ProfileContext } from '@/services/profileContext'
import { createOperationId } from '@/services/operationId'
import { reportPersonalDataFailure } from '@/services/personalDataStatus'

let baseline: ProfileSnapshot['streaming'] = null
let owner: ProfileContext | null = null
let queued: Record<string, unknown> = {}
let pending: { id: string; body: object } | null = null
let running: Promise<void> | null = null
let blocked = false
const dirty = ref(false)
/** Unsaved player choices, held only in memory and retried explicitly after failures. */
export const pendingPlayerPreferences = readonly(dirty)

/** Accepts a confirmed document unless a local player write still owns its revision.
 * @param snapshot Complete server snapshot.
 * @param context Owning profile generation.
 * @returns Whether server player choices may replace runtime choices.
 */
export function acceptPlayerPreferences(snapshot: ProfileSnapshot, context: ProfileContext): boolean {
  if (!isCurrentProfile(context) || dirty.value) return false
  if (owner?.generation === context.generation && baseline && snapshot.streaming && snapshot.streaming.revision < baseline.revision) return false
  owner = context
  baseline = snapshot.streaming ? JSON.parse(JSON.stringify(snapshot.streaming)) : null
  blocked = false
  return true
}

/** Queues only changed player fields against the last confirmed revision. @param values Changed fields. @param context Owning player generation. */
export function queuePlayerPreferences(values: Record<string, unknown>, context: ProfileContext): void {
  if (!isCurrentProfile(context) || owner?.generation !== context.generation || !baseline || blocked) return
  Object.assign(queued, JSON.parse(JSON.stringify(values)))
  dirty.value = true
  if (!pending && !running) void flushPlayerPreferences()
}

/** Serializes player writes and retries uncertain requests with the exact same body and UUID. */
export async function flushPlayerPreferences(): Promise<void> {
  if (running) return running
  const context = owner
  if (!context || !baseline || blocked || !isCurrentProfile(context)) return
  const task = async () => {
    try {
      while (isCurrentProfile(context) && (pending || Object.keys(queued).length)) {
        if (!pending) {
          const values = { ...JSON.parse(JSON.stringify(baseline!.values)), ...queued }
          queued = {}
          pending = { id: createOperationId(), body: { streaming: { expected_revision: baseline!.revision, values } } }
        }
        const request = pending
        const result = await profileCall<OperationView>('save_preferences', { ...request.body, operation_id: request.id }, true, context)
        if (result.state !== 'completed') throw new ProfileApiError('operation_in_progress', 'Player preferences are awaiting confirmation.')
        const confirmed = await profileCall<ProfileSnapshot>('get', {}, false, context)
        if (confirmed.pending_operation) throw new ProfileApiError('operation_in_progress', 'Player preferences are awaiting confirmation.')
        if (Object.keys(queued).length && confirmed.streaming?.revision !== result.result?.streaming_revision) {
          throw new ProfileApiError('revision_conflict', 'Player preferences changed elsewhere. Reload server settings before making another choice.')
        }
        baseline = confirmed.streaming
        pending = null
      }
      if (isCurrentProfile(context)) dirty.value = false
    } catch (error) {
      if (!isCurrentProfile(context)) return
      if (error instanceof ProfileApiError && ['revision_conflict', 'invalid_data', 'unsupported_schema_version', 'operation_id_conflict'].includes(error.code)) {
        pending = null
        queued = {}
        dirty.value = false
        blocked = true
      }
      reportPersonalDataFailure(error)
    }
  }
  running = task()
  const current = running
  try { await current } finally { if (running === current) running = null }
}

registerProfileFlusher(async () => {
  await Promise.race([flushPlayerPreferences(), new Promise<void>((resolve) => setTimeout(resolve, 5_000))])
})
window.addEventListener('arachnea-profile-context', () => {
  baseline = null
  owner = null
  queued = {}
  pending = null
  running = null
  blocked = false
  dirty.value = false
})
