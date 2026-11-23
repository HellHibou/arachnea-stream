import { readonly, shallowRef } from 'vue'
import { hasProfile, isUncertainProfileWrite, profileCall, profileWrite, type StreamingSnapshot } from '@/services/profileApi'
import { requireProfileContext, type ProfileContext } from '@/services/profileContext'

const current = shallowRef<string | null>(null)
let snapshot: StreamingSnapshot['streaming'] = null
/** Confirmed profile-owned default home; null opens the aggregated landing page. */
export const profileDefaultHomeId = readonly(current)
/** Reads the default home from the streaming snapshot. @param context Captured profile context. */
export async function loadDefaultHome(context?: ProfileContext): Promise<string | null> {
  if (!hasProfile()) return null
  const data = await profileCall<StreamingSnapshot>('get', {}, false, context ?? requireProfileContext())
  snapshot = data.streaming
  current.value = typeof snapshot?.values.defaultHomeId === 'string' ? snapshot.values.defaultHomeId : null
  return current.value
}
/** Saves a profile-local default reference with an expected settings revision. @param id Home UUID, or null. */
export async function saveDefaultHome(id: string | null): Promise<void> {
  const context = requireProfileContext()
  if (!snapshot) await loadDefaultHome(context)
  if (!snapshot) throw new Error('Initialize streaming preferences before choosing a default home.')
  try {
    await profileWrite('save_preferences', { streaming: { expected_revision: snapshot.revision, values: { ...snapshot.values, defaultHomeId: id } } }, 'settings', context)
  } catch (error) {
    snapshot = null
    if (isUncertainProfileWrite(error) && await loadDefaultHome(context).catch(() => undefined) === id) return
    throw error
  }
  await loadDefaultHome(context)
}
window.addEventListener('arachnea-profile-context', () => { current.value = null; snapshot = null })
