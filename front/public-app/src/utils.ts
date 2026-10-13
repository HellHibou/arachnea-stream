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
