import { readonly, shallowRef } from 'vue'

import type { ScraperExecutionError } from '@/types/scraperError'

/** Session-global queue consumed by the notification UI introduced in point 6. */
const notifications = shallowRef<ScraperExecutionError[]>([])
window.addEventListener('arachnea-session-context', () => { notifications.value = [] })

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

/**
 * Logs and queues a client-reported error through the generic notification stack.
 * @param operation Operation label displayed in the localized summary.
 * @param message Technical diagnostic shown in the collapsed details.
 */
export function reportFrontendErrorNotification(operation: string, message: string): void {
  const error: ScraperExecutionError = {
    code: nextFrontendErrorCode(), operation, source: null, origin: 'frontend', message,
  }
  console.error('Frontend operation error', error)
  enqueueErrorNotification(error)
}

/**
 * Queues a frontend diagnostic unless an identical one is still visible.
 * @param operation Operation label displayed in the localized summary.
 * @param message Technical diagnostic shown in the collapsed details.
 */
export function reportFrontendErrorOnce(operation: string, message: string): void {
  if (notifications.value.some((notification) => notification.origin === 'frontend' && notification.message === message)) return
  reportFrontendErrorNotification(operation, message)
}

let lastFrontendErrorMillis = 0
let frontendErrorSequence = 0

/** Generates a monotonic browser-local diagnostic code matching the backend format. */
function nextFrontendErrorCode(): string {
  const currentMillis = Date.now()

  if (currentMillis <= lastFrontendErrorMillis) {
    frontendErrorSequence += 1
    if (frontendErrorSequence > 99) {
      lastFrontendErrorMillis += 1
      frontendErrorSequence = 0
    }
  } else {
    lastFrontendErrorMillis = currentMillis
    frontendErrorSequence = 0
  }

  return `ARACHNEA_E${lastFrontendErrorMillis}${String(frontendErrorSequence).padStart(2, '0')}`
}
