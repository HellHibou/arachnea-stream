import { readonly, shallowRef } from 'vue'
import { ProfileApiError } from '@/services/profileApi'

const issue = shallowRef<{ code: string; message: string } | null>(null)
/** Visible personal-write failure; drafts and queued progress remain in memory. */
export const personalDataIssue = readonly(issue)
/** Publishes a safe personal-data failure. @param error Transport, validation or revision rejection. */
export function reportPersonalDataFailure(error: unknown): void {
  if (error instanceof ProfileApiError && ['profile_context_changed', 'personal_data_changed'].includes(error.code)) return
  issue.value = { code: error instanceof ProfileApiError ? error.code : 'storage_unavailable', message: error instanceof Error ? error.message : String(error) }
}
/** Clears the displayed failure after explicit acknowledgement or a successful retry. */
export function clearPersonalDataFailure(): void { issue.value = null }
window.addEventListener('arachnea-profile-context', clearPersonalDataFailure)
