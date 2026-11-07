/** Aliases from ISO 639-2/B and legacy codes to ISO 639-1 codes. */
const LANGUAGE_CODE_ALIASES: Record<string, string> = {
  chi: 'zh',
  deu: 'de',
  dut: 'nl',
  eng: 'en',
  fra: 'fr',
  fre: 'fr',
  ger: 'de',
  ita: 'it',
  jpn: 'ja',
  nld: 'nl',
  por: 'pt',
  spa: 'es',
  zho: 'zh',
}

/**
 * Normalizes a BCP-47 language code for persistence and comparisons.
 *
 * @param value Raw language code.
 * @returns Lowercase code with aliases and `_` separators resolved, or `null` when invalid.
 */
export function normalizeLanguageCode(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null
  }

  const normalizedValue = value.trim().toLowerCase().replace(/_/g, '-')
  if (!normalizedValue) {
    return null
  }

  const [rawBase = '', ...subtags] = normalizedValue.split('-')
  const base = LANGUAGE_CODE_ALIASES[rawBase] ?? rawBase
  if (!/^[a-z]{2,3}$/.test(base) || subtags.some((subtag) => !/^[a-z0-9]{1,8}$/.test(subtag))) {
    return null
  }

  return [base, ...subtags].join('-')
}

/**
 * Returns the base language subtag of a normalized language code.
 *
 * @param language Normalized language code.
 * @returns Base language code, or `null` when the value is empty.
 */
export function getBaseLanguageCode(language: string | null): string | null {
  return language ? language.split('-')[0] ?? null : null
}

/**
 * Generates an RFC 4122 version 4 UUID.
 *
 * Prefers `crypto.randomUUID()` when available, which requires a secure
 * context (HTTPS, localhost or 127.0.0.1) that plain HTTP LAN hosts such as
 * `http://192.168.x.x:8080` do not provide (notably on Firefox/Windows).
 * Falls back to `crypto.getRandomValues()` (available in every context),
 * then to `Math.random()` as a last resort so home creation never crashes.
 *
 * @returns Lowercase UUID string (e.g. `xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`).
 */
export function createUuid(): string {
  try {
    const cryptoRef = typeof globalThis.crypto !== 'undefined' ? globalThis.crypto : undefined
    if (typeof cryptoRef?.randomUUID === 'function') return cryptoRef.randomUUID()
    if (typeof cryptoRef?.getRandomValues === 'function') {
      const bytes = new Uint8Array(16)
      cryptoRef.getRandomValues(bytes)
      bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x40
      bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80
      const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
      return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
    }
  } catch {
    // Fall through to the Math.random() fallback below.
  }
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (char) => {
    const random = Math.floor(Math.random() * 16)
    return (char === 'x' ? random : (random & 0x3) | 0x8).toString(16)
  })
}
