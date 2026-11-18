import { getAppBaseDir } from '@/services/baseUrl'
import { isDesktopApp } from '@/services/rustify/desktop'
import { getStoreItem, removeStoreItem, setStoreItem } from '@/services/storage/persistence'

/** Maximum stored identifier length accepted from local resume state. */
const MAX_IDENTIFIER_LENGTH = 128

/**
 * Encodes a server identity as unpadded base64url.
 *
 * @param value Canonical server or installation identity.
 * @returns URL-safe key fragment without padding.
 */
function encodeIdentity(value: string): string {
  const bytes = new TextEncoder().encode(value)
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

/**
 * Returns the stable identity of the server serving this application instance.
 *
 * Browser deployments use the canonical origin and application root. Native
 * desktop builds use one installation-scoped identity because they do not
 * address a remote origin.
 *
 * @returns Opaque identity used only for resume keys.
 */
export function profileServerIdentity(): string {
  if (isDesktopApp()) return 'native'
  const base = new URL(getAppBaseDir())
  return `${base.origin}${base.pathname.replace(/\/$/, '')}`
}

/**
 * Builds the local key suffix for the last profile of one account.
 *
 * @param userId Stable account UUID.
 * @returns Local key suffix, with no secret.
 */
function lastProfileKey(userId: string): string {
  return `resume.${encodeIdentity(profileServerIdentity())}.${encodeIdentity(userId)}.lastProfileId`
}

/**
 * Reads one locally stored identifier with basic shape validation.
 *
 * @param key Storage key suffix.
 * @returns Stored identifier or null.
 */
function readIdentifier(key: string): string | null {
  const value = getStoreItem<unknown>(key, null)
  return typeof value === 'string' && value.length > 0 && value.length <= MAX_IDENTIFIER_LENGTH ? value : null
}

/**
 * Stores the last selected profile for one account.
 *
 * @param userId Confirmed account UUID.
 * @param profileId Confirmed profile UUID.
 */
export function rememberLastProfileId(userId: string, profileId: string): void {
  setStoreItem(lastProfileKey(userId), profileId)
}

/** Returns the last selected profile for one account, if any. @param userId Confirmed account UUID. */
export function readLastProfileId(userId: string): string | null {
  return readIdentifier(lastProfileKey(userId))
}

/** Removes the last selected profile for one account. @param userId Confirmed account UUID. */
export function forgetLastProfileId(userId: string): void {
  removeStoreItem(lastProfileKey(userId))
}
