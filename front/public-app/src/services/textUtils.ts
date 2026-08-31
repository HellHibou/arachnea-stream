/**
 * Converts a plain text string into a URL-friendly slug by:
 * - Lower-casing the input
 * - Removing diacritics from accented characters (`é` -> `e`)
 * - Replacing every character outside `[0-9a-z]` with `_`
 * - Stripping trailing underscores
 *
 * @param text - The raw text input
 * @returns The normalized slug usable inside a route segment, possibly empty
 */
export function buildUrlSlug(text: string): string {
  return text
    .toLocaleLowerCase()
    .normalize('NFD')
    .replace(/\p{Diacritic}/gu, '')
    .replace(/[^0-9a-z]/gu, '_')
    .replace(/_+$/u, '')
}

/**
 * Converts a plain text string into safe HTML by:
 * - Escaping HTML special characters to prevent XSS
 * - Converting `\n` (newlines) into `<br>` tags
 * - Preserving multiple spaces as `&nbsp;` sequences
 *
 * @param text - The raw plain text input
 * @returns The HTML-safe string with `<br>` and `&nbsp;` markers
 */
export function textToHtml(text: string): string {
  if (!text) {
    return ''
  }

  return _escapeHtml(text)
    .replace(/  +/g, (spaces) => '\u00A0'.repeat(spaces.length))
    .replace(/\n/g, '<br>')
}

/**
 * Builds an HTML entity string, e.g. _entity('amp') returns '&'.
 *
 * @param name - The entity name (e.g., 'amp', 'lt', 'gt').
 * @returns The HTML entity string.
 */
function _entity(name: string): string {
  return '\x26' + name + '\x3b'
}

/**
 * Escapes HTML special characters (&, <, >, ", ') in a string.
 *
 * @param text - The string to escape.
 * @returns The escaped string safe for HTML.
 */
function _escapeHtml(text: string): string {
  const amp = _entity('amp')
  const lt = _entity('lt')
  const gt = _entity('gt')
  const quot = _entity('quot')
  const apos = _entity('#039')

  const chars: Record<string, string> = {
    '\x26': amp,
    '\x3c': lt,
    '\x3e': gt,
    '\x22': quot,
    '\x27': apos,
  }

  return text.replace(/[&<>"']/g, (char) => chars[char] ?? char)
}
