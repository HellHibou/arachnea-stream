import { readonly, shallowRef } from 'vue'

/**
 * Shape of a backend communication error displayed as a popup notification.
 */
export interface BackendCommunicationError {
  /** Unique notification identifier used as list key and dismissal target. */
  id: string
  /** Stable machine-readable error code from the API layer. */
  code: string
  /** Human-readable error message. */
  message: string
  /** HTTP status code when available. */
  status?: number
}

/** Maximum number of queued notifications retained at once. */
const MAX_QUEUED_NOTIFICATIONS = 10
/** Counter guaranteeing unique notification identifiers. */
let notificationCounter = 0

/** Global ordered notification queue shared by every composable instance. */
const notifications = shallowRef<BackendCommunicationError[]>([])

/**
 * Global composable exposing the backend error queue consumed by the error
 * notification stack.
 */
export function useErrorNotifications() {
  /**
   * Appends a communication error to the queue, skipping exact duplicates.
   */
  function pushError(error: Omit<BackendCommunicationError, 'id'>): void {
    const isDuplicate = notifications.value.some(
      (notification) => notification.code === error.code && notification.message === error.message,
    )
    if (isDuplicate) {
      return
    }

    notificationCounter += 1
    const notification: BackendCommunicationError = {
      ...error,
      id: `backend-error-${notificationCounter}`,
    }
    notifications.value = [...notifications.value, notification].slice(-MAX_QUEUED_NOTIFICATIONS)
  }

  /** Removes one notification from the queue. */
  function dismiss(id: string): void {
    notifications.value = notifications.value.filter((notification) => notification.id !== id)
  }

  /** Removes every queued notification. */
  function clearErrors(): void {
    notifications.value = []
  }

  return {
    notifications: readonly(notifications),
    pushError,
    dismiss,
    clearErrors,
  }
}
