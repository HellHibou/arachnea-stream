import catalog from './languages.json'

/** Writing direction of a language. */
export type LanguageDirection = 'ltr' | 'rtl'

/** One language entry shared by every Arachnea frontend. */
export interface Language {
  /** ISO 639-1 two-letter code. */
  code: string
  /** ISO 639-2 three-letter terminology code. */
  alpha3: string
  /** ISO 639-2 three-letter bibliographic code when it differs from `alpha3`. */
  alpha3Bibliographic?: string
  /** English display name. */
  name: string
  /** Display name in the language itself. */
  nativeName: string
  /** Flag emoji for the language. */
  flag: string
  /** Writing direction of the language. */
  direction: LanguageDirection
}

/** Shape of the shared language catalog file. */
interface LanguageCatalog {
  /** Application defaults expressed with catalog codes. */
  defaults: {
    /** Fallback interface language code. */
    uiLanguage: string
    /** Fallback ordered subtitle language priority. */
    subtitleLanguagePriority: string[]
    /** Fallback configured subtitle languages. */
    subtitleLanguages: string[]
  }
  /** Supported languages sorted by English name. */
  languages: Language[]
}

const languageCatalog = catalog as LanguageCatalog

/** All supported languages sorted by English name. */
export const LANGUAGES: readonly Language[] = languageCatalog.languages

/** Fallback interface language code. */
export const DEFAULT_LANGUAGE_CODE = languageCatalog.defaults.uiLanguage

/** Fallback ordered subtitle language priority. */
export const DEFAULT_SUBTITLE_LANGUAGE_PRIORITY: readonly string[] =
  languageCatalog.defaults.subtitleLanguagePriority

/** Fallback configured subtitle languages. */
export const DEFAULT_SUBTITLE_LANGUAGES: readonly string[] =
  languageCatalog.defaults.subtitleLanguages

/** First configured subtitle language, used as a language-input placeholder. */
export const DEFAULT_SUBTITLE_LANGUAGE =
  DEFAULT_SUBTITLE_LANGUAGES[0] ?? DEFAULT_LANGUAGE_CODE

/** Language lookups by ISO 639-1 code. */
const languageByCode = new Map(LANGUAGES.map((language) => [language.code, language]))
/** Language lookups by ISO 639-2 terminology and bibliographic codes. */
const languageByAlpha3 = new Map<string, Language>()
for (const language of LANGUAGES) {
  languageByAlpha3.set(language.alpha3, language)
  if (language.alpha3Bibliographic) {
    languageByAlpha3.set(language.alpha3Bibliographic, language)
  }
}

/**
 * Looks up a language by its ISO 639-1 or ISO 639-2 code.
 *
 * @param code Raw language code, in any supported ISO form.
 * @returns Matching catalog entry, or `null` when the code is unknown.
 */
export function getLanguage(code: string | null | undefined): Language | null {
  if (!code) {
    return null
  }

  const normalizedCode = code.trim().toLowerCase()
  if (!normalizedCode) {
    return null
  }

  return languageByCode.get(normalizedCode) ?? languageByAlpha3.get(normalizedCode) ?? null
}

/**
 * Checks whether a value is a language code present in the catalog.
 *
 * @param code Raw language code.
 * @returns True when the catalog knows the code.
 */
export function isKnownLanguageCode(code: string | null | undefined): boolean {
  return getLanguage(code) !== null
}

/**
 * Returns the English name of a language.
 *
 * @param code Raw language code.
 * @returns English name, or `null` when the code is unknown.
 */
export function getLanguageEnglishName(code: string | null | undefined): string | null {
  return getLanguage(code)?.name ?? null
}

/**
 * Returns the native name of a language.
 *
 * @param code Raw language code.
 * @returns Native name, or `null` when the code is unknown.
 */
export function getLanguageNativeName(code: string | null | undefined): string | null {
  return getLanguage(code)?.nativeName ?? null
}

/**
 * Returns the flag emoji of a language.
 *
 * @param code Raw language code.
 * @returns Flag emoji, or `null` when the code is unknown.
 */
export function getLanguageFlag(code: string | null | undefined): string | null {
  return getLanguage(code)?.flag ?? null
}

/**
 * Formats a language for display as `<flag> <English name> - <native name>`.
 *
 * @param code Raw language code.
 * @returns Formatted label, or the raw code when the language is unknown.
 */
export function formatLanguageLabel(code: string): string {
  const language = getLanguage(code)
  if (!language) {
    return code
  }

  return `${language.flag} ${language.name} - ${language.nativeName}`
}

/**
 * Normalizes a BCP-47 language code for persistence and comparisons.
 *
 * Known ISO 639-2 codes are converted to their ISO 639-1 equivalent while
 * unknown but well-formed codes are preserved for source-specific tracks.
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
  const base = getLanguage(rawBase)?.code ?? rawBase
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
