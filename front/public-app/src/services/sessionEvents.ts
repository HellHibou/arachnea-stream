/** Monotonic context used to discard replies from a previous public account. */
let generation = 0
let contentAllowed = false

/** Returns the current request context generation. */
export function sessionGeneration(): number { return generation }
/** Returns whether content callbacks may initialize local caches. */
export function isSessionContentAllowed(): boolean { return contentAllowed }
/** Invalidates all in-memory consumers after an account/restriction change. @param allowed Whether content may render. */
export function changeSessionContext(allowed: boolean): void {
  contentAllowed = allowed
  generation += 1
  window.dispatchEvent(new Event('arachnea-session-context'))
}
/** Requests reauthentication after a protected API rejection. @param code Stable backend error code. */
export function reportSessionFailure(code: string): void {
  if (['session_required', 'session_expired', 'session_revoked', 'password_change_required'].includes(code)) {
    window.dispatchEvent(new CustomEvent('arachnea-session-invalid', { detail: code }))
  }
}
