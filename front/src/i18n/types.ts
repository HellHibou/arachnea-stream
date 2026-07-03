/** Language entry in the locale index. */
export interface LocaleIndexLanguage {
  /** ISO language code (e.g., 'en', 'fr'). */
  code: string
  /** Display label for the language. */
  label: string
}

/** Locale index containing available languages and the default language. */
export interface LocaleIndex {
  /** Default language code to use when no preference is set. */
  defaultLanguage: string
  /** List of all available languages. */
  languages: LocaleIndexLanguage[]
}

/** Leaf node in the translation tree - either a string or a key-value map of string to string. */
export type TranslationLeaf = string | Record<string, string>

/** Nested translation dictionary structure for organizing translations. */
export interface TranslationDictionary {
  [key: string]: TranslationLeaf | TranslationDictionary
}

/** Parameters that can be interpolated into translation strings. */
export interface TranslationParams {
  [key: string]: string | number | boolean | null | undefined
}
