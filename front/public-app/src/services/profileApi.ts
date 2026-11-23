import { resolveAppPath } from '@/services/baseUrl'
import { captureSessionFailureReporter, isSessionContentAllowed, sessionGeneration } from '@/services/sessionEvents'
import { getProfileContext, isCurrentProfile, requireProfileContext, type ProfileContext } from '@/services/profileContext'
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
/** Complete profile snapshot consumed before mounting content views. */
export interface ProfileSnapshot { profile: ProfileView; common: VersionedSettings<CommonPreferences>; streaming: VersionedSettings<Record<string, unknown>> | null; onboarding: OnboardingView[] }
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
  const reportSessionFailure = captureSessionFailureReporter()
  const epoch = mutationEpoch
  const payload: Record<string, unknown> = { ...values, profile_id: context.profileId }
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
const inFlight = new Set<string>()

/** Identifies a network or storage failure whose write outcome requires a fresh read. */
export function isUncertainProfileWrite(error: unknown): boolean {
  return !(error instanceof ProfileApiError) || error.code === 'storage_unavailable'
}

/** Compares JSON preference values independently of object key order. */
export function sameProfileValues(left: unknown, right: unknown): boolean {
  if (left === right) return true
  if (!left || !right || typeof left !== 'object' || typeof right !== 'object') return false
  if (Array.isArray(left) !== Array.isArray(right)) return false
  const first = left as Record<string, unknown>
  const second = right as Record<string, unknown>
  const keys = Object.keys(first)
  return keys.length === Object.keys(second).length
    && keys.every((key) => Object.prototype.hasOwnProperty.call(second, key) && sameProfileValues(first[key], second[key]))
}

/**
 * Serializes mutations for one resource and invalidates overlapping reads.
 * @param operation Profile mutation name.
 * @param body Complete mutation body excluding the profile UUID.
 * @param identity Profile-local resource identity.
 * @param context Captured profile context.
 */
export async function profileWrite<T>(operation: string, body: object, identity: string, context = requireProfileContext()): Promise<T> {
  const key = `${context.generation}:${identity}`
  if (inFlight.has(key)) throw new ProfileApiError('operation_in_progress', 'A change is already in progress.')
  inFlight.add(key)
  mutationEpoch += 1
  try {
    return await profileCall<T>(operation, body, true, context)
  } finally {
    mutationEpoch += 1
    inFlight.delete(key)
  }
}

/**
 * Calls a profile operation that only needs the public account session.
 *
 * Used by profile listing, creation, snapshots and onboarding before an active
 * profile exists. The caller supplies `profile_id` when required.
 *
 * @param operation Command suffix under `profiles/`.
 * @param values Query values or complete mutation body.
 * @param write Whether this is a POST mutation.
 * @returns Confirmed data; replies from another account generation are rejected.
 */
export async function profileSessionCall<T>(operation: string, values: object = {}, write = false): Promise<T> {
  if (!isSessionContentAllowed()) throw new ProfileApiError('session_required', 'Sign in before managing profiles.')
  const reportSessionFailure = captureSessionFailureReporter()
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

/** Sends a profile mutation using the current account session. */
export function profileSessionWrite<T>(operation: string, body: object): Promise<T> {
  return profileSessionCall<T>(operation, body, true)
}

/** Returns an empty collection only when no profile has been selected yet. */
export function hasProfile(): boolean { return getProfileContext() !== null }
