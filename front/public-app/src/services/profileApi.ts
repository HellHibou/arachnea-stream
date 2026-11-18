import { resolveAppPath } from '@/services/baseUrl'
import { reportSessionFailure } from '@/services/sessionEvents'
import { getProfileContext, isCurrentProfile, requireProfileContext, type ProfileContext } from '@/services/profileContext'
import { createOperationId } from '@/services/operationId'
import type { TauriWindow } from '@/services/rustify/desktop'

/** Safe profile-service error, retaining the code for explicit conflict/retry handling. */
export class ProfileApiError extends Error {
  constructor(public readonly code: string, message: string) { super(message) }
}
/** Versioned collection returned by streaming endpoints. */
export interface ProfileCollection<T> { items: T[]; revision: number }
/** Snapshot subset used to maintain the server-owned default home. */
export interface StreamingSnapshot { streaming: { revision: number; schema_version: number; values: Record<string, unknown> } | null }

/**
 * Calls the REST/Tauri profile API without catalogue caches or local persistence.
 * @param operation Command suffix.
 * @param values Query or mutation values excluding the profile UUID.
 * @param write Whether this request is a POST mutation.
 * @param context Immutable account/profile context captured before asynchronous work.
 * @returns Confirmed data; replies from another generation are rejected.
 */
export async function profileCall<T>(operation: string, values: object = {}, write = false, context: ProfileContext = requireProfileContext()): Promise<T> {
  if (!isCurrentProfile(context)) throw new ProfileApiError('profile_context_changed', 'The selected profile changed.')
  const epoch = mutationEpoch
  const payload: Record<string, unknown> = operation === 'operation' ? { ...values } : { ...values, profile_id: context.profileId }
  const tauri = window as TauriWindow
  const invoke = tauri.__TAURI__?.core?.invoke ?? tauri.__TAURI__?.invoke
  let envelope: { data: T; error: { code: string; message: string } | null }
  try {
    if (invoke) {
      envelope = await invoke(`profiles/${operation}`, payload)
    } else {
      const base = (import.meta.env.VITE_RUSTIFY_API_BASE_URL ?? resolveAppPath('api')).replace(/\/$/, '')
      const query = new URLSearchParams(Object.entries(payload).map(([key, value]) => [key, typeof value === 'string' ? value : JSON.stringify(value)]))
      const response = await fetch(`${base}/profiles/${operation}${write ? '' : `?${query}`}`, {
        method: write ? 'POST' : 'GET', credentials: 'same-origin', cache: 'no-store',
        headers: write ? { 'Content-Type': 'application/json' } : {},
        body: write ? JSON.stringify(payload) : undefined,
      })
      envelope = await response.json()
      if (!response.ok && !envelope.error) throw new ProfileApiError('storage_unavailable', 'Personal data service is unavailable.')
    }
  } catch (error) {
    if (!isCurrentProfile(context)) throw new ProfileApiError('profile_context_changed', 'The selected profile changed.')
    throw error
  }
  if (!isCurrentProfile(context)) throw new ProfileApiError('profile_context_changed', 'The selected profile changed.')
  if (!write && epoch !== mutationEpoch) throw new ProfileApiError('personal_data_changed', 'Personal data changed during this refresh.')
  if (envelope.error) {
    reportSessionFailure(envelope.error.code)
    throw new ProfileApiError(envelope.error.code, envelope.error.message)
  }
  return envelope.data
}

let mutationEpoch = 0
const pending = new Map<string, { operation: string; body: string; id: string }>()
/**
 * Retains retry identity after uncertain writes and refuses replacing an unacknowledged request.
 * @param operation Profile mutation name.
 * @param body Complete mutation body excluding operation/profile identifiers.
 * @param identity Profile-local resource identity for serialization and uncertain retries.
 * @param context Captured profile context.
 */
export async function profileWrite<T>(operation: string, body: object, identity: string, context = requireProfileContext()): Promise<T> {
  const key = `${context.generation}:${identity}`
  const serialized = JSON.stringify(body)
  let request = pending.get(key)
  if (request && (request.operation !== operation || request.body !== serialized)) throw new ProfileApiError('operation_in_progress', 'Retry the pending change before applying another edit.')
  if (!request) { request = { operation, body: serialized, id: createOperationId() }; pending.set(key, request) }
  mutationEpoch += 1
  try {
    const result = await profileCall<T>(operation, { ...body, operation_id: request.id }, true, context)
    if (pending.get(key) === request) pending.delete(key)
    mutationEpoch += 1
    return result
  } catch (error) {
    if (error instanceof ProfileApiError && !['storage_unavailable', 'profile_context_changed'].includes(error.code) && pending.get(key) === request) pending.delete(key)
    throw error
  }
}

/** Returns an empty collection only when no profile has been selected yet. */
export function hasProfile(): boolean { return getProfileContext() !== null }
window.addEventListener('arachnea-profile-context', () => pending.clear())
