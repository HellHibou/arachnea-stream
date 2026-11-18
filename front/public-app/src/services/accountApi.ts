import { resolveAppPath } from '@/services/baseUrl'
import type { TauriWindow } from '@/services/rustify/desktop'

/** Safe account identity returned by the server. */
export interface PublicAccount { user_id: string; login: string; must_change_password: boolean }
/** Session metadata; secrets are carried only by cookies or the native broker. */
export interface PublicSession { authenticated: boolean; account: PublicAccount | null; expires_at: number | null; persistent: boolean }
interface Envelope<T> { data: T; error: { code: string; message: string } | null }

/** Structured account error for localized rendering. */
export class AccountApiError extends Error {
  constructor(public readonly code: string, message: string) { super(message) }
}

/** Calls a public account operation without the catalogue GET/ETag bridge. @param operation Command suffix. @param body JSON mutation, or absent for GET. */
export async function accountCall<T>(operation: string, body?: object): Promise<T> {
  const tauri = window as TauriWindow
  const invoke = tauri.__TAURI__?.core?.invoke ?? tauri.__TAURI__?.invoke
  let envelope: Envelope<T>
  if (invoke) {
    envelope = await invoke<Envelope<T>>(`accounts/${operation}`, body as Record<string, unknown> ?? {})
  } else {
    const base = import.meta.env.VITE_RUSTIFY_API_BASE_URL ?? resolveAppPath('api')
    const response = await fetch(`${base}/accounts/${operation}`, {
      method: body ? 'POST' : 'GET', credentials: 'same-origin', cache: 'no-store',
      headers: body ? { 'Content-Type': 'application/json' } : {},
      body: body ? JSON.stringify(body) : undefined,
    })
    envelope = await response.json() as Envelope<T>
    if (!response.ok && !envelope.error) throw new AccountApiError('storage_unavailable', 'Account service is unavailable.')
  }
  if (envelope.error) throw new AccountApiError(envelope.error.code, envelope.error.message)
  return envelope.data
}
