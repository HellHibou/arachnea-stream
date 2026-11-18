import { readonly, shallowRef } from 'vue'
import { isSessionContentAllowed, sessionGeneration } from '@/services/sessionEvents'

/** Immutable identity carried by every personal request and queued write. */
export interface ProfileContext { profileId: string; generation: number; session: number }
const active = shallowRef<ProfileContext | null>(null)
let generation = 0
let selection = 0
const flushers = new Set<() => Promise<void>>()

/** Read-only profile context; the selection/onboarding flow owns activation. */
export const activeProfileContext = readonly(active)
/** Returns the current profile generation, including an unselected context. */
export function profileGeneration(): number { return generation }
/** Returns the selected profile without granting authorization; the server verifies ownership. */
export function getProfileContext(): ProfileContext | null { return active.value }
/** Rejects personal actions until an unrestricted session and explicit profile are available. */
export function requireProfileContext(): ProfileContext {
  if (!active.value || !isSessionContentAllowed()) throw new Error('Select a profile before saving personal data.')
  return active.value
}
/** Checks whether a delayed callback still belongs to the active account/profile. @param context Captured immutable context. */
export function isCurrentProfile(context: ProfileContext): boolean {
  return active.value?.generation === context.generation && context.session === sessionGeneration() && isSessionContentAllowed()
}
/** Registers a best-effort flush before explicit profile switching. @param flush Serialized pending-write callback. */
export function registerProfileFlusher(flush: () => Promise<void>): void { flushers.add(flush) }
/**
 * Activates an explicitly selected, server-validated profile; no automatic selection or creation.
 * @param profileId Selected common UUID, or null to clear the selection.
 * @returns Resolves after pending writes have had a chance to flush under their old UUID.
 */
export async function setActiveProfileContext(profileId: string | null): Promise<void> {
  const session = sessionGeneration()
  const request = ++selection
  await Promise.allSettled([...flushers].map((flush) => flush()))
  if (session !== sessionGeneration() || request !== selection) return
  publish(profileId)
}
/** Publishes a new generation and clears all profile-bound consumers. @param profileId Common UUID or no selection. */
function publish(profileId: string | null): void {
  generation += 1
  active.value = profileId ? { profileId, generation, session: sessionGeneration() } : null
  window.dispatchEvent(new Event('arachnea-profile-context'))
}
window.addEventListener('arachnea-session-context', () => { selection += 1; publish(null) })
