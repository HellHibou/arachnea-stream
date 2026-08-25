import { buildUrlSlug } from '@/services/textUtils'
import type { HomeCategory, HomeCategorySource } from '@/types/home'

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

/**
 * Selection values used to build an entry detail route parameter.
 *
 * The title is optional and only feeds the informative URL slug; it never
 * participates in the encoded payload, keeping bookmark keys stable.
 */
export interface EntryRouteTarget extends EntryRoutePayload {
  /** Display title of the entry used for the informative URL slug. */
  title?: string | null
}

/** Payload for aggregated category routes. */
export interface CategoryRoutePayload extends VersionedRoutePayload {
  /** Display label of the category. */
  label: string
  /** Source descriptors required to load the category catalog. */
  sources: HomeCategorySource[]
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
 * Builds the entry detail route parameter with an informative title slug.
 *
 * The parameter is `<slug>-<base64url payload>` where the slug derives from
 * the entry title. The slug is purely informative: the encoded payload stays
 * identical to the bookmark key produced by `encodeEntryRoutePayload`. The
 * slug never contains a dash, so the first dash unambiguously separates it
 * from the token.
 *
 * @param target Entry selection values including the optional display title.
 * @returns Route-safe entry parameter.
 */
export function encodeEntryRouteParam(target: EntryRouteTarget): string {
  const slug = buildUrlSlug(target.title ?? '') || 'entry'

  return `${slug}-${encodeEntryRoutePayload({
    source: target.source,
    entryUrl: target.entryUrl,
  })}`
}

/**
 * Decodes an entry detail base64url token.
 *
 * Accepts both a bare base64url token and a `<slug>-<token>` route segment;
 * the slug prefix, when present, is ignored.
 *
 * @param token Route segment received from Vue Router.
 * @returns Valid entry payload, or null when the token is invalid.
 */
export function decodeEntryRoutePayload(token: string): EntryRoutePayload | null {
  const separatorIndex = token.indexOf('-')
  const payloadToken = separatorIndex >= 0 ? token.slice(separatorIndex + 1) : token
  const payload = decodeRoutePayload(payloadToken)

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
 * Builds the category route parameter from a category descriptor.
 *
 * The parameter is `<slug>-<base64url payload>` where the slug is the
 * lower-cased label with every character outside `[0-9a-z]` replaced by `_`.
 * The slug is purely informative; the base64url token carries the data. Both
 * sides are unambiguously separated by the first dash because neither the
 * slug nor the base64url alphabet contains one.
 *
 * @param category Category selected from the home catalog.
 * @returns Route-safe category parameter.
 */
export function encodeCategoryRouteParam(category: HomeCategory): string {
  const slug = buildUrlSlug(category.label) || 'category'
  const payload: CategoryRoutePayload = {
    v: 1,
    label: category.label,
    sources: category.sources,
  }

  return `${slug}-${encodeRoutePayload(payload)}`
}

/**
 * Decodes a category route parameter into its payload.
 *
 * @param segment Route segment received from Vue Router.
 * @returns Valid category payload, or null when the parameter is invalid.
 */
export function decodeCategoryRoutePayload(segment: string): CategoryRoutePayload | null {
  const separatorIndex = segment.indexOf('-')

  if (separatorIndex < 0) {
    return null
  }

  const payload = decodeRoutePayload(segment.slice(separatorIndex + 1))

  if (!isRouteRecord(payload) || payload.v !== 1 || !Array.isArray(payload.sources)) {
    return null
  }

  return {
    v: 1,
    label: readNonEmptyString(payload.label) ?? '',
    sources: payload.sources.flatMap(readHomeCategorySource),
  }
}

/**
 * Sanitizes one decoded source descriptor, keeping only string-valued pairs.
 *
 * @param value Decoded source entry.
 * @returns A single-entry list with the sanitized source, or an empty list when invalid.
 */
function readHomeCategorySource(value: unknown): HomeCategorySource[] {
  if (!isRouteRecord(value)) {
    return []
  }

  const source: HomeCategorySource = {}

  Object.entries(value).forEach(([key, entryValue]) => {
    if (typeof entryValue === 'string') {
      source[key] = entryValue
    }
  })

  return [source]
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
