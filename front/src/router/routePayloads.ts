/** Base interface for versioned route payloads. */
interface VersionedRoutePayload {
  /** Version identifier for the payload format. */
  v: 1
}

/** Payload for entry detail routes. */
export interface EntryRoutePayload {
  /** Backend source identifier for the entry. */
  source: string
  /** Absolute entry URL used to fetch entry details. */
  entryUrl: string
}

/** Payload for live channel routes. */
export interface LiveRoutePayload extends VersionedRoutePayload {
  /** Backend source identifier for the live channel. */
  source: string
  /** Source-specific live channel identifier. */
  channel: string
}

/** Text encoder used for payload serialization. */
const textEncoder = new TextEncoder()
/** Text decoder used for payload deserialization. */
const textDecoder = new TextDecoder()

/**
 * Encodes an entry detail route payload into a base64url token.
 *
 * @param payload Entry detail values required to reload the page directly.
 * @returns Route-safe opaque token.
 */
export function encodeEntryRoutePayload(payload: EntryRoutePayload): string {
  return encodeRoutePayload(payload)
}

/**
 * Decodes an entry detail base64url token.
 *
 * @param token Route segment received from Vue Router.
 * @returns Valid entry payload, or null when the token is invalid.
 */
export function decodeEntryRoutePayload(token: string): EntryRoutePayload | null {
  const payload = decodeRoutePayload(token)

  if (!isRouteRecord(payload)) {
    return null
  }

  const source = readNonEmptyString(payload.source)
  const entryUrl = readNonEmptyString(payload.entryUrl)

  if (!source || !entryUrl) {
    return null
  }

  return {
    source,
    entryUrl,
  }
}

/**
 * Encodes a live route payload into a base64url token.
 *
 * @param payload Live values required to select a channel after a refresh.
 * @returns Route-safe opaque token.
 */
export function encodeLiveRoutePayload(payload: LiveRoutePayload): string {
  return encodeRoutePayload(payload)
}

/**
 * Decodes a live base64url token.
 *
 * @param token Route segment received from Vue Router.
 * @returns Valid live payload, or null when the token is invalid.
 */
export function decodeLiveRoutePayload(token: string): LiveRoutePayload | null {
  const payload = decodeRoutePayload(token)

  if (!isRouteRecord(payload) || payload.v !== 1) {
    return null
  }

  const source = readNonEmptyString(payload.source)
  const channel = readNonEmptyString(payload.channel)

  if (!source || !channel) {
    return null
  }

  return {
    v: 1,
    source,
    channel,
  }
}

/**
 * Encodes a payload object into a base64url token.
 *
 * @param payload - Object to encode.
 * @returns Base64url-encoded token.
 */
function encodeRoutePayload(payload: object): string {
  const json = JSON.stringify(payload)
  const bytes = textEncoder.encode(json)
  let binary = ''

  bytes.forEach((byte) => {
    binary += String.fromCharCode(byte)
  })

  return btoa(binary)
    .replace(/\+/gu, '-')
    .replace(/\//gu, '_')
    .replace(/=+$/u, '')
}

/**
 * Decodes a base64url token into a payload object.
 *
 * @param token - Base64url-encoded token.
 * @returns Decoded payload object, or null when the token is invalid.
 */
function decodeRoutePayload(token: string): unknown {
  const normalizedToken = token.trim()

  if (!normalizedToken || !/^[A-Za-z0-9_-]+$/u.test(normalizedToken)) {
    return null
  }

  const base64 = normalizedToken
    .replace(/-/gu, '+')
    .replace(/_/gu, '/')
    .padEnd(Math.ceil(normalizedToken.length / 4) * 4, '=')

  try {
    const binary = atob(base64)
    const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0))
    return JSON.parse(textDecoder.decode(bytes))
  } catch {
    return null
  }
}

/**
 * Type guard to check if a value is a plain object record.
 *
 * @param value - Value to check.
 * @returns True if the value is a plain object record.
 */
function isRouteRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/**
 * Extracts a non-empty trimmed string from an unknown value.
 *
 * @param value - Value to normalize.
 * @returns Trimmed non-empty string, or null when not a valid non-empty string.
 */
function readNonEmptyString(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null
  }

  const normalizedValue = value.trim()
  return normalizedValue || null
}
