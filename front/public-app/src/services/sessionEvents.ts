/** Monotonic context used to discard replies from a previous public account. */
let generation = 0
let credentialGeneration = 0
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
/** Records a successful account operation that replaced or cleared the transport credential. */
export function changeSessionCredential(): void { credentialGeneration += 1 }

/** Captures a reporter that ignores rejections for credentials replaced since the request started. */
export function captureSessionFailureReporter(): (code: string) => void {
  const credential = credentialGeneration
  return (code) => {
    if (credential === credentialGeneration && ['session_required', 'session_expired', 'session_revoked', 'password_change_required'].includes(code)) {
      window.dispatchEvent(new CustomEvent('arachnea-session-invalid', { detail: code }))
    }
  }
}
