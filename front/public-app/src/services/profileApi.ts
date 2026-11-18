import { resolveAppPath } from '@/services/baseUrl'
import { isSessionContentAllowed, reportSessionFailure, sessionGeneration } from '@/services/sessionEvents'
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
/** Authoritative profile identity returned by the common store. */
export interface ProfileView { profile_id: string; user_id: string; name: string; avatar_id: string; order: number; state: 'creating' | 'active' | 'deleting'; revision: number; created_at: number; updated_at: number }
/** Common preferences owned by the account profile. */
export interface CommonPreferences { language: string | null; theme: string }
/** Versioned settings document shared by both stores. */
export interface VersionedSettings<T> { schema_version: number; revision: number; values: T }
/** Per-domain onboarding state. */
export interface OnboardingView { profile_id: string; scope: string; state: 'pending' | 'completed'; revision: number }
/** Durable operation state used to resume interrupted guides. */
export interface OperationView { operation_id: string; kind: string; profile_id: string | null; state: 'pending' | 'running' | 'retry_pending' | 'completed'; step: string; retryable: boolean; result: Record<string, unknown> | null }
/** Complete profile snapshot consumed before mounting content views. */
export interface ProfileSnapshot { profile: ProfileView; common: VersionedSettings<CommonPreferences>; streaming: VersionedSettings<Record<string, unknown>> | null; onboarding: OnboardingView[]; pending_operation: OperationView | null }
/** Account profile list with the live quota. */
export interface ProfileList { profiles: ProfileView[]; max_profiles_per_user: number; used_slots: number }
/** Embedded avatar catalogue reference. */
export interface AvatarCatalogueEntry { avatar_id: string; url: string; mime_type: string }
/** Authenticated embedded avatar catalogue. */
export interface AvatarCatalogue { schema_version: number; fallback_avatar_id: string; avatars: AvatarCatalogueEntry[] }

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

/**
 * Calls a profile operation that only needs the public account session.
 *
 * Used by profile listing, creation, snapshots, onboarding and operation
 * polling, before an active profile exists. Values are sent unchanged; the
 * caller supplies `profile_id` and `operation_id` when the operation needs them.
 *
 * @param operation Command suffix under `profiles/`.
 * @param values Query values or complete mutation body.
 * @param write Whether this is a POST mutation.
 * @returns Confirmed data; replies from another account generation are rejected.
 */
export async function profileSessionCall<T>(operation: string, values: object = {}, write = false): Promise<T> {
  if (!isSessionContentAllowed()) throw new ProfileApiError('session_required', 'Sign in before managing profiles.')
  const session = sessionGeneration()
  const payload = { ...values }
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
    if (session !== sessionGeneration()) throw new ProfileApiError('profile_context_changed', 'The account changed.')
    throw error
  }
  if (session !== sessionGeneration() || !isSessionContentAllowed()) throw new ProfileApiError('profile_context_changed', 'The account changed.')
  if (envelope.error) {
    reportSessionFailure(envelope.error.code)
    throw new ProfileApiError(envelope.error.code, envelope.error.message)
  }
  return envelope.data
}

/**
 * Sends one profile mutation with a stable client operation identity.
 *
 * @param operation Command suffix under `profiles/`.
 * @param body Complete mutation body excluding the operation UUID.
 * @param operationId Reused UUID for retrying an uncertain write.
 * @returns Confirmed operation view or endpoint-specific data.
 */
export function profileSessionWrite<T>(operation: string, body: object, operationId = createOperationId()): Promise<T> {
  return profileSessionCall<T>(operation, { ...body, operation_id: operationId }, true)
}

/**
 * Waits for a durable operation to reach a terminal state.
 *
 * @param operationId Client operation UUID returned by a mutation.
 * @param attempts Maximum status reads.
 * @returns Completed operation, or the last observed state.
 */
export async function pollProfileOperation(operationId: string, attempts = 20): Promise<OperationView | null> {
  let last: OperationView | null = null
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      last = await profileSessionCall<OperationView>('operation', { operation_id: operationId })
    } catch (error) {
      const transient = !(error instanceof ProfileApiError) || ['storage_unavailable', 'operation_in_progress'].includes(error.code)
      if (!transient) throw error
    }
    if (last?.state === 'completed') return last
    await new Promise((resolve) => setTimeout(resolve, Math.min(250 + attempt * 100, 1_500)))
  }
  return last
}

/** Returns an empty collection only when no profile has been selected yet. */
export function hasProfile(): boolean { return getProfileContext() !== null }
window.addEventListener('arachnea-profile-context', () => pending.clear())
