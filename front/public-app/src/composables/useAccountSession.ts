import { computed, onMounted, onScopeDispose, readonly, shallowRef } from 'vue'
import { accountCall, AccountApiError, type PublicSession } from '@/services/accountApi'
import { changeSessionContext, changeSessionCredential } from '@/services/sessionEvents'
import { getAppBaseDir } from '@/services/baseUrl'

const session = shallowRef<PublicSession | null>(null)
const busy = shallowRef(false)
const errorCode = shallowRef<string | null>(null)
const restored = shallowRef(false)
const reloadRequired = shallowRef(false)
const justLoggedIn = shallowRef(false)
let requestSequence = 0
let restoreStarted = false
let pendingRejection: string | null = null
let channel: BroadcastChannel | null = null

/** Applies confirmed session metadata and invalidates the previous account context. @param next Confirmed session or anonymous state. */
function apply(next: PublicSession | null): void {
  const before = `${session.value?.account?.user_id ?? ''}:${session.value?.account?.must_change_password ?? ''}`
  const after = `${next?.account?.user_id ?? ''}:${next?.account?.must_change_password ?? ''}`
  session.value = next
  if (before !== after) changeSessionContext(Boolean(next?.authenticated && !next.account?.must_change_password))
}

/** Clears the current session and shows why. @param code Account error code. */
function invalidate(code: string): void {
  ++requestSequence
  justLoggedIn.value = false
  apply(null)
  errorCode.value = code
  if (code === 'password_change_required') reloadRequired.value = true
}

/** Restores session state once per document, discarding stale responses. */
async function restore(): Promise<void> {
  if (restoreStarted) return
  restoreStarted = true
  const sequence = ++requestSequence
  try {
    const next = await accountCall<PublicSession>('session')
    if (sequence === requestSequence) {
      apply(next)
      errorCode.value = null
      reloadRequired.value = false
    }
  } catch (error) {
    if (sequence !== requestSequence) return
    errorCode.value = error instanceof AccountApiError ? error.code : 'storage_unavailable'
    reloadRequired.value = errorCode.value === 'storage_unavailable'
  } finally {
    restored.value = true
  }
}

/** Performs a session mutation while preserving visible errors. @param operation Account command. @param body Non-persisted form values. */
async function mutate(operation: string, body: object): Promise<void> {
  busy.value = true
  errorCode.value = null
  const sequence = ++requestSequence
  try {
    const next = await accountCall<PublicSession>(operation, body)
    if (sequence !== requestSequence) return
    changeSessionCredential()
    apply(operation === 'logout' ? null : next)
    reloadRequired.value = false
    justLoggedIn.value = operation !== 'logout'
    channel?.postMessage('changed')
  } catch (error) {
    if (sequence === requestSequence) {
      const code = error instanceof AccountApiError ? error.code : 'storage_unavailable'
      errorCode.value = code
      if (operation === 'change_password' && code === 'storage_unavailable') {
        invalidate(code)
        reloadRequired.value = true
      } else if (code.startsWith('session_')) invalidate(code)
      else if (pendingRejection) invalidate(pendingRejection)
    }
    throw error
  } finally { pendingRejection = null; busy.value = false }
}

/** Provides account state/actions, without mixing profile forms or local storage. */
export function useAccountSession() {
  return {
    session: readonly(session), busy: readonly(busy), errorCode: readonly(errorCode), restored: readonly(restored),
    justLoggedIn: readonly(justLoggedIn), reloadRequired: readonly(reloadRequired),
    consumeJustLoggedIn: () => { justLoggedIn.value = false },
    ready: computed(() => Boolean(session.value?.authenticated && !session.value.account?.must_change_password)),
    /** Signs in with in-memory form values. @param login Login identifier. @param password Password. */
    login: (login: string, password: string) => mutate('login', { login, password }),
    /** Replaces a temporary password before content access. @param current_password Current secret. @param new_password Replacement secret. */
    changePassword: (current_password: string, new_password: string) => mutate('change_password', { current_password, new_password }),
    /** Revokes the current session and closes the authenticated context. */
    logout: () => mutate('logout', {}),
  }
}

/** Restores the session on mount and listens for invalidation. */
export function useAccountSessionMonitor(): void {
  let unlistenNative: (() => void) | undefined
  let disposed = false
  /** Clears stale content when another tab changes the shared account. */
  const changed = () => {
    invalidate('session_changed')
    reloadRequired.value = true
  }
  /** Defers a rejection until an in-flight account operation establishes the current state. @param event Rejection with the API error code. */
  const invalid = (event: Event) => {
    const code = (event as CustomEvent<string>).detail
    if (busy.value) pendingRejection = code
    else invalidate(code)
  }
  onMounted(() => {
    window.addEventListener('arachnea-session-invalid', invalid)
    const native = (window as unknown as { __TAURI__?: { webview?: { getCurrentWebview: () => { listen: (name: string, listener: () => void) => Promise<() => void> } } } }).__TAURI__?.webview
    if (native) {
      void native.getCurrentWebview().listen('arachnea-public-session-changed', changed).then((unlisten) => {
        if (disposed) unlisten()
        else unlistenNative = unlisten
      }).catch((error: unknown) => {
        console.warn('Could not listen for native session changes.', error)
      }).finally(() => {
        if (!disposed) void restore()
      })
      return
    }
    if ('BroadcastChannel' in window) {
      channel = new BroadcastChannel(`arachnea.accounts.${getAppBaseDir()}`)
      channel.addEventListener('message', changed)
    }
    void restore()
  })
  onScopeDispose(() => {
    disposed = true; unlistenNative?.()
    channel?.close(); channel = null
    window.removeEventListener('arachnea-session-invalid', invalid)
  })
}
