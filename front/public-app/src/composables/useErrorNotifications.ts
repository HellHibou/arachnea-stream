import { readonly, shallowRef } from 'vue'

import type { ScraperExecutionError } from '@/types/scraperError'

/** Session-global queue consumed by the notification UI introduced in point 6. */
const notifications = shallowRef<ScraperExecutionError[]>([])

/** Adds one structured execution error to the global notification queue. */
export function enqueueErrorNotification(error: ScraperExecutionError): void {
  notifications.value = [...notifications.value, error]
}

/** Provides read-only notification state and explicit queue actions. */
export function useErrorNotifications() {
  /** Removes one notification using its correlation code. */
  function dismiss(code: string): void {
    notifications.value = notifications.value.filter((notification) => notification.code !== code)
  }

  return {
    notifications: readonly(notifications),
    enqueue: enqueueErrorNotification,
    dismiss,
  }
}
