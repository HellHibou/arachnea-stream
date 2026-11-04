import type { JsonRecord } from '../types'

/**
 * Reads a nested property from an unknown JSON-like object.
 *
 * @param value Root value to inspect.
 * @param path Nested property path.
 * @returns Nested value or `null` when the path does not exist.
 */
export function readPath(value: unknown, ...path: string[]): unknown {
  let current: unknown = value

  for (const segment of path) {
    if (!isJsonRecord(current)) {
      return null
    }

    current = current[segment]
  }

  return current
}

/**
 * Collects typed string values from a backend scalar or array.
 *
 * @param value Unknown backend node.
 * @returns Flat list of trimmed non-empty strings.
 */
export function readStringList(value: unknown): string[] {
  if (typeof value === 'string') {
    const normalized = normalizeString(value)
    return normalized ? [normalized] : []
  }

  if (Array.isArray(value)) {
    return value.flatMap((item) => {
      if (typeof item !== 'string') {
        return []
      }

      const normalized = normalizeString(item)
      return normalized ? [normalized] : []
    })
  }

  return []
}

/**
 * Reads a string-only record from an unknown backend object shape.
 *
 * @param value Unknown backend object that may contain scraper scalar nodes.
 * @returns Plain string record containing the first non-empty value for each key.
 */
export function readStringMap(value: unknown): Record<string, string> {
  if (!isJsonRecord(value)) {
    return {}
  }

  const output: Record<string, string> = {}

  Object.entries(value).forEach(([key, entryValue]) => {
    const normalizedValue = typeof entryValue === 'string' ? normalizeString(entryValue) : null

    if (normalizedValue) {
      output[key] = normalizedValue
    }
  })

  return output
}

/**
 * Reads primitive backend values into request parameter strings.
 *
 * @param value Unknown backend object used as source/request params.
 * @returns String-only parameter record accepted by backend calls.
 */
export function readStringParamMap(value: unknown): Record<string, string> {
  if (!isJsonRecord(value)) {
    return {}
  }

  const output: Record<string, string> = {}

  Object.entries(value).forEach(([key, entryValue]) => {
    if (typeof entryValue === 'string') {
      const normalizedValue = normalizeString(entryValue)
      if (normalizedValue) {
        output[key] = normalizedValue
      }
      return
    }

    if (typeof entryValue === 'number' && Number.isFinite(entryValue)) {
      output[key] = String(entryValue)
      return
    }

    if (typeof entryValue === 'boolean') {
      output[key] = String(entryValue)
    }
  })

  return output
}

/**
 * Collects plain-object records from a serialized scraper node.
 *
 * @param value Unknown backend node.
 * @returns Flat list of JSON-like records.
 */
export function readRecordList(value: unknown): JsonRecord[] {
  if (Array.isArray(value)) {
    return value.filter(isJsonRecord)
  }

  return isJsonRecord(value) ? [value] : []
}

/**
 * Reads the first available string value from a list of backend nodes.
 *
 * @param values Candidate backend nodes.
 * @returns First non-empty string value.
 */
export function firstNonEmptyString(values: unknown[]): string | null {
  for (const value of values) {
    const candidate = readStringList(value)[0]

    if (candidate) {
      return candidate
    }
  }

  return null
}

/**
 * Normalizes ordered ISO alpha-2 proxy countries.
 *
 * Accepts direct strings and Scrapyfy scalar nodes (`{ _: "FR" }`) inside the
 * array because nested static object arrays preserve scalar items in node form.
 *
 * @param countries Raw country list returned by a resolver descriptor.
 * @returns Uppercase, deduplicated country codes in declaration order.
 */
export function normalizeProxyCountries(countries: unknown): string[] {
  return Array.isArray(countries)
    ? countries.reduce<string[]>((result, country) => {
        const value = typeof country === 'string'
          ? country
          : firstNonEmptyString([isJsonRecord(country) ? country._ : null])

        if (!value) {
          return result
        }

        const normalizedCountry = value.trim().toUpperCase()
        if (/^[A-Z]{2}$/.test(normalizedCountry) && !result.includes(normalizedCountry)) {
          result.push(normalizedCountry)
        }
        return result
      }, [])
    : []
}

/**
 * Reads a typed numeric backend value.
 *
 * @param value Backend value.
 * @returns Number when the backend emitted one.
 */
export function firstNumber(value: unknown): number | null {
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : null
  }

  return null
}

/**
 * Reads a backend duration expressed either as a seconds string or as a
 * number of seconds, and returns the seconds count as a string suitable for
 * `formatDurationLabel`.
 *
 * Backend scrapers may emit `duration` as a number (e.g. anime-sama via
 * `normalize_duration`) or as a string (e.g. frenchanimes, papadustream).
 * `firstNonEmptyString` only keeps strings, so a numeric value would be
 * silently dropped and the duration would never display.
 *
 * @param value Raw backend duration node.
 * @returns Seconds count as a string when valid.
 */
export function firstDurationInSeconds(value: unknown): string | null {
  const asNumber = firstNumber(value)

  if (asNumber !== null && Number.isInteger(asNumber) && asNumber >= 0) {
    return String(asNumber)
  }

  return firstNonEmptyString([value])
}

/**
 * Normalizes one numeric backend value into a positive integer.
 *
 * @param value Parsed numeric backend value.
 * @returns Positive integer when available.
 */
export function toPositiveInteger(value: number | null): number | null {
  if (value === null) {
    return null
  }

  const normalizedValue = Math.trunc(value)
  return Number.isFinite(normalizedValue) && normalizedValue > 0 ? normalizedValue : null
}

/**
 * Normalizes one numeric backend value into a non-negative integer.
 *
 * @param value Parsed numeric backend value.
 * @returns Non-negative integer when available.
 */
export function toNonNegativeInteger(value: number | null): number | null {
  if (value === null) {
    return null
  }

  const normalizedValue = Math.trunc(value)
  return Number.isFinite(normalizedValue) && normalizedValue >= 0 ? normalizedValue : null
}

/**
 * Reads a typed boolean backend value.
 *
 * @param value Backend value.
 * @returns Boolean when available, otherwise `false`.
 */
export function readBoolean(value: unknown): boolean {
  if (typeof value === 'boolean') {
    return value
  }

  return false
}

/**
 * Reads a backend boolean flag that may be encoded as a string.
 *
 * Static YAML scraper entries emit every value as a string, so a flag declared
 * as `true` in a source YAML reaches the frontend as `"true"`. Typed backend
 * booleans are accepted as-is.
 *
 * @param value Backend value.
 * @returns `true` only when the backend explicitly enabled the flag.
 */
export function readBooleanFlag(value: unknown): boolean {
  if (typeof value === 'boolean') {
    return value
  }

  if (typeof value === 'string') {
    const normalized = value.trim().toLowerCase()
    return normalized === 'true' || normalized === '1'
  }

  return false
}

/**
 * Reads a boolean only when the backend explicitly emitted a typed boolean.
 *
 * @param value Backend value.
 * @returns Boolean, or `null` when the field is absent/unknown.
 */
export function readOptionalBoolean(value: unknown): boolean | null {
  if (typeof value === 'boolean') {
    return value
  }

  return null
}

/**
 * Reads a media link from either the flat scraper key shape or a nested object shape.
 *
 * @param record Raw backend record.
 * @param flatKey Flat field name such as `img/poster`.
 * @param nestedPath Fallback nested path when the payload is object-shaped.
 * @returns First resolved raw link string.
 */
export function readFirstLink(record: JsonRecord, flatKey: string, nestedPath: string[]): string | null {
  return firstNonEmptyString([
    readPath(record, flatKey, 'link'),
    readPath(record, ...nestedPath, 'link'),
  ])
}

/**
 * Resolves a media asset URL against the source base URL when the backend returns a relative path.
 *
 * @param value Raw asset URL returned by the backend.
 * @param source Source name associated with the asset.
 * @returns Absolute asset URL when it can be resolved.
 */
export function resolveAssetUrl(value: string | null, source: string | null): string | null {
  if (!value) {
    return null
  }

  if (/^https?:\/\//i.test(value)) {
    return value
  }

  if (value.startsWith('//')) {
    return `https:${value}`
  }

  return value
}

/**
 * Resolves an entry URL against the source base URL when the backend returns a relative path.
 *
 * @param value Raw entry link returned by the backend search query.
 * @param source Source name associated with the entry.
 * @returns Absolute entry URL when it can be resolved.
 */
export function resolveEntryUrl(value: string | null, source: string | null): string | null {
  if (!value) {
    return null
  }

  const normalizedValue = normalizeString(value)
  if (!normalizedValue) {
    return null
  }

  if (/^https?:\/\//i.test(normalizedValue)) {
    return normalizedValue
  }

  if (normalizedValue.startsWith('//')) {
    return `https:${normalizedValue}`
  }

  return normalizedValue
}

/**
 * Produces a stable frontend id from the backend result contents.
 *
 * @param index Search result index.
 * @param link Canonical entry URL when available.
 * @param title Display title used as a fallback.
 * @param source Backend source associated with the item.
 * @returns Stable card identifier.
 */
export function buildMediaId(
  index: number,
  link: string | null,
  title: string | null,
  source: string | null = null,
): string {
  const baseValue = [normalizeString(source ?? ''), link, title, `media-${index + 1}`]
    .filter((value): value is string => Boolean(value))
    .join('-')
  const normalizedBaseValue = slugify(baseValue) || `media-${index + 1}`

  return `${normalizedBaseValue}-${index + 1}`
}

/**
 * Prefixes a backend identifier with its source to avoid cross-source collisions.
 *
 * @param id Raw identifier returned by the backend.
 * @param source Backend source associated with the item.
 * @returns Scoped identifier or `null` when no id is available.
 */
export function buildScopedId(id: string | null, source: string | null): string | null {
  const normalizedId = normalizeString(id ?? '')
  if (!normalizedId) {
    return null
  }

  const normalizedSource = normalizeString(source ?? '')
  return normalizedSource ? `${normalizedSource}:${normalizedId}` : normalizedId
}

/**
 * Normalizes a display string by trimming surrounding whitespace.
 *
 * @param value Raw string value.
 * @returns Trimmed string or `null` when empty.
 */
export function normalizeString(value: string): string | null {
  const normalized = value.trim()
  return normalized.length > 0 ? normalized : null
}

/**
 * Creates a URL-safe identifier fragment from a backend string.
 *
 * @param value Raw identifier source.
 * @returns Slugified identifier fragment.
 */
export function slugify(value: string): string {
  return value
    .toLocaleLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
}

/**
 * Checks whether an unknown value is a plain object record.
 *
 * @param value Value to inspect.
 * @returns `true` when the value can be accessed as a key/value record.
 */
export function isJsonRecord(value: unknown): value is JsonRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}
