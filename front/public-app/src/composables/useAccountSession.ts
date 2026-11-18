import { createOperationId } from '@/services/operationId'
import { computed, onMounted, onScopeDispose, readonly, shallowRef } from 'vue'
import { accountCall, AccountApiError, type PublicSession } from '@/services/accountApi'
import { changeSessionContext } from '@/services/sessionEvents'
import { getAppBaseDir } from '@/services/baseUrl'

const session = shallowRef<PublicSession | null>(null)
const busy = shallowRef(false)
const errorCode = shallowRef<string | null>(null)
const restored = shallowRef(false)
const justLoggedIn = shallowRef(false)
let requestSequence = 0
let restorePromise: Promise<void> | null = null
let channel: BroadcastChannel | null = null

/** Applies confirmed session metadata and invalidates the previous account context. @param next Confirmed session or anonymous state. */
function apply(next: PublicSession | null): void {
  const before = `${session.value?.account?.user_id ?? ''}:${session.value?.account?.must_change_password ?? ''}`
  const after = `${next?.account?.user_id ?? ''}:${next?.account?.must_change_password ?? ''}`
  session.value = next
  if (before !== after) changeSessionContext(Boolean(next?.authenticated && !next.account?.must_change_password))
}

/** Restores session state with stale-response protection and no local token. */
async function restore(): Promise<void> {
  if (restorePromise) return restorePromise
  const sequence = ++requestSequence
  restorePromise = (async () => {
    try {
      const next = await accountCall<PublicSession>('session')
      if (sequence === requestSequence) { apply(next); errorCode.value = null }
    } catch (error) {
      if (sequence !== requestSequence) return
      errorCode.value = error instanceof AccountApiError ? error.code : 'storage_unavailable'
      if (error instanceof AccountApiError && error.code.startsWith('session_')) apply(null)
    } finally {
      if (sequence === requestSequence) restored.value = true
      restorePromise = null
      if (sequence !== requestSequence && !busy.value) void restore()
    }
  })()
  return restorePromise
}

/** Performs a session mutation while preserving visible errors. @param operation Account command. @param body Non-persisted form values. */
async function mutate(operation: string, body: object): Promise<void> {
  busy.value = true
  errorCode.value = null
  const sequence = ++requestSequence
  try {
    const next = await accountCall<PublicSession>(operation, body)
    if (sequence !== requestSequence) return
    apply(operation === 'logout' ? null : next)
    channel?.postMessage('changed')
  } catch (error) {
    if (sequence === requestSequence) errorCode.value = error instanceof AccountApiError ? error.code : 'storage_unavailable'
    throw error
  } finally { busy.value = false; if (sequence !== requestSequence) void restore() }
}

/** Provides account state/actions, without mixing profile forms or local storage. */
export function useAccountSession() {
  return {
    session: readonly(session), busy: readonly(busy), errorCode: readonly(errorCode), restored: readonly(restored),
    justLoggedIn: readonly(justLoggedIn),
    consumeJustLoggedIn: () => { justLoggedIn.value = false },
    ready: computed(() => Boolean(session.value?.authenticated && !session.value.account?.must_change_password)),
    restore,
    /** Signs in with in-memory form values. @param login Login identifier. @param password Password. */
    login: async (login: string, password: string) => {
      await mutate('login', { login, password })
      justLoggedIn.value = true
    },
    /** Replaces a temporary password before content access. @param current_password Current secret. @param new_password Replacement secret. */
    changePassword: async (current_password: string, new_password: string) => {
      await mutate('change_password', { operation_id: createOperationId(), current_password, new_password })
      justLoggedIn.value = true
    },
    /** Revokes the current session and closes the authenticated context. */
    logout: () => mutate('logout', { operation_id: createOperationId() }),
  }
}

/** Installs one gate's browser/native-tab session revalidation lifecycle. */
export function useAccountSessionMonitor(): void {
  let timer: ReturnType<typeof setInterval> | undefined
  let unlistenNative: (() => void) | undefined
  let disposed = false
  /** Revalidates only while visible, avoiding writes racing a form submit. */
  const refresh = () => { if (!busy.value && document.visibilityState === 'visible') void restore() }
  /** Closes stale content immediately before resolving a remote account switch. */
  const changed = () => { ++requestSequence; apply(null); void restore() }
  /** Handles protected API failures before a stale callback renders. */
  const invalid = () => { ++requestSequence; apply(null); if (!busy.value) void restore() }
  onMounted(() => {
    const native = (window as unknown as { __TAURI__?: { event?: { listen: (name: string, listener: () => void) => Promise<() => void> } } }).__TAURI__?.event
    if (native) void native.listen('arachnea-public-session-changed', changed).then((unlisten) => { if (disposed) unlisten(); else unlistenNative = unlisten })
    if ('BroadcastChannel' in window) {
      channel = new BroadcastChannel(`arachnea.accounts.${getAppBaseDir()}`)
      channel.addEventListener('message', changed)
    }
    window.addEventListener('focus', refresh)
    document.addEventListener('visibilitychange', refresh)
    window.addEventListener('arachnea-session-invalid', invalid)
    timer = setInterval(refresh, 30_000)
    void restore()
  })
  onScopeDispose(() => {
    disposed = true; unlistenNative?.()
    channel?.close(); channel = null
    if (timer) clearInterval(timer)
    window.removeEventListener('focus', refresh)
    document.removeEventListener('visibilitychange', refresh)
    window.removeEventListener('arachnea-session-invalid', invalid)
  })
}
