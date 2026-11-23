import { readonly, ref } from 'vue'
import { isUncertainProfileWrite, profileCall, profileWrite, ProfileApiError, sameProfileValues, type ProfileSnapshot } from '@/services/profileApi'
import { isCurrentProfile, registerProfileFlusher, type ProfileContext } from '@/services/profileContext'
import { reportPersonalDataFailure } from '@/services/personalDataStatus'

let baseline: ProfileSnapshot['streaming'] = null
let owner: ProfileContext | null = null
let queued: Record<string, unknown> = {}
let pending: { expected_revision: number; values: Record<string, unknown> } | null = null
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

/** Serializes player writes and reads back uncertain outcomes before continuing. */
export async function flushPlayerPreferences(): Promise<void> {
  if (running) return running
  const context = owner
  if (!context || !baseline || blocked || !isCurrentProfile(context)) return
  const task = async () => {
    try {
      while (isCurrentProfile(context) && baseline && (pending || Object.keys(queued).length)) {
        let revision: number | null = null
        if (!pending) {
          pending = { expected_revision: baseline.revision, values: { ...baseline.values, ...queued } }
          queued = {}
          try {
            const result = await profileWrite<{ streaming_revision: number }>('save_preferences', { streaming: pending }, 'settings', context)
            revision = result.streaming_revision
          } catch (error) {
            if (!isUncertainProfileWrite(error)) throw error
          }
        }
        const request = pending
        if (!request || !isCurrentProfile(context)) return
        const confirmed = await profileCall<ProfileSnapshot>('get', {}, false, context)
        if (!isCurrentProfile(context)) return
        const matches = revision === null
          ? sameProfileValues(confirmed.streaming?.values, request.values)
          : confirmed.streaming?.revision === revision
        if (!matches && (revision === null || Object.keys(queued).length)) {
          baseline = confirmed.streaming
          throw new ProfileApiError('revision_conflict', 'Player preferences differ from the server state.')
        }
        baseline = confirmed.streaming
        pending = null
      }
      if (isCurrentProfile(context)) dirty.value = false
    } catch (error) {
      if (!isCurrentProfile(context)) return
      if (!isUncertainProfileWrite(error)) {
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
