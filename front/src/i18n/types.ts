export interface LocaleIndexLanguage {
  code: string
  label: string
}

export interface LocaleIndex {
  defaultLanguage: string
  languages: LocaleIndexLanguage[]
}

export type TranslationLeaf = string | Record<string, string>

export interface TranslationDictionary {
  [key: string]: TranslationLeaf | TranslationDictionary
}

export interface TranslationParams {
  [key: string]: string | number | boolean | null | undefined
}
