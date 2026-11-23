const listeners = new Set<() => void>()
let pending: number | undefined

/** Schedules one dispatch after the browser has delivered both foreground events. */
function schedule(): void {
  window.clearTimeout(pending)
  if (document.visibilityState !== 'visible') return
  pending = window.setTimeout(() => {
    if (document.visibilityState === 'visible') listeners.forEach((refresh) => refresh())
  }, 100)
}

/**
 * Groups focus and visibility events from a single return to the window.
 * @param refresh Callback to run while the document is visible.
 * @returns Listener cleanup, including any pending refresh.
 */
export function onWindowForeground(refresh: () => void): () => void {
  if (!listeners.size) {
    window.addEventListener('focus', schedule)
    document.addEventListener('visibilitychange', schedule)
  }
  listeners.add(refresh)

  return () => {
    listeners.delete(refresh)
    if (listeners.size) return
    window.clearTimeout(pending)
    window.removeEventListener('focus', schedule)
    document.removeEventListener('visibilitychange', schedule)
  }
}
