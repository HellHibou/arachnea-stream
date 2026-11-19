import { readonly, shallowRef } from 'vue'
import { ProfileApiError } from '@/services/profileApi'
import { reportFrontendErrorNotification, useErrorNotifications } from '@/composables/useErrorNotifications'
import { t } from '@/i18n'

const issue = shallowRef<{ code: string; message: string } | null>(null)
const { notifications } = useErrorNotifications()
/** Retained personal-write failure for retry controls; drafts and queued progress remain in memory. */
export const personalDataIssue = readonly(issue)
/** Queues a generic notification and retains personal-write retry state. @param error Transport, validation or revision rejection. */
export function reportPersonalDataFailure(error: unknown): void {
  if (error instanceof ProfileApiError && ['profile_context_changed', 'personal_data_changed'].includes(error.code)) return
  issue.value = { code: error instanceof ProfileApiError ? error.code : 'storage_unavailable', message: error instanceof Error ? error.message : String(error) }
  const operation = t('personalData.operation')
  const message = `[${issue.value.code}] ${issue.value.message}`
  if (notifications.value.some((notification) => notification.operation === operation && notification.message === message)) return
  reportFrontendErrorNotification(operation, message)
}
/** Clears personal-write retry status after acknowledgement, successful retry or profile change. */
export function clearPersonalDataFailure(): void { issue.value = null }
window.addEventListener('arachnea-profile-context', clearPersonalDataFailure)
