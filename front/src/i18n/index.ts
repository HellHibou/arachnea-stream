import { computed, readonly, shallowRef, watch } from 'vue'

import { useStorage } from '@/services/storage'
import type {
  LocaleIndex,
  LocaleIndexLanguage,
  TranslationDictionary,
  TranslationParams,
} from './types'

// Legacy messages to convert to i18n
export const MSG_LIVE_TV = 'Live TV';
export const MSG_LIVE = 'Live'; // live.direct
export const MSG_LIVE_PLAYING = 'Direct en cours.'
export const MSG_EPISODES = 'Episodes';

/** Default fallback language code. */
const DEFAULT_LANGUAGE = 'en'
/** Special language code for automatic browser language detection. */
const AUTO_LANGUAGE = 'auto'
/** Base path for locale JSON files. */
const LOCALE_BASE_PATH = '/locales'

/** List of languages available for selection. */
const availableLanguages = shallowRef<LocaleIndexLanguage[]>([
  { code: DEFAULT_LANGUAGE, label: 'English' },
])
/** Reactive default language setting. */
const defaultLanguage = shallowRef(DEFAULT_LANGUAGE)
/** Currently selected language preference, or null for auto. */
const selectedLanguage = shallowRef<string | null>(null)
/** Resolved active language after applying preferences and fallbacks. */
const resolvedLanguage = shallowRef(DEFAULT_LANGUAGE)
/** Fallback translation dictionary (English). */
const fallbackMessages = shallowRef<TranslationDictionary>({})
/** Active translation dictionary for the selected language. */
const activeMessages = shallowRef<TranslationDictionary>({})
/** Whether translations are currently being loaded. */
const isLoading = shallowRef(false)
/** Error message from the last translation loading attempt, or null if successful. */
const errorMessage = shallowRef<string | null>(null)
/** Promise for tracking i18n initialization. */
let initPromise: Promise<void> | null = null

/**
 * Initializes locale index and dictionaries for the current session.
 *
 * @returns Promise resolved once fallback and active dictionaries are loaded.
 */
export function initializeI18n(): Promise<void> {
  if (initPromise) {
    return initPromise
  }

  initPromise = loadInitialI18n()
  return initPromise
}

/**
 * Provides the shared localization state and translation helpers.
 *
 * @returns i18n state and utility functions.
 */
export function useI18n() {
  return {
    availableLanguages: readonly(availableLanguages),
    selectedLanguage: readonly(selectedLanguage),
    resolvedLanguage: readonly(resolvedLanguage),
    languageOptions: computed(() => [
      { code: AUTO_LANGUAGE, label: t('settings.language.auto') },
      ...availableLanguages.value,
    ]),
    isLoading: readonly(isLoading),
    errorMessage: readonly(errorMessage),
    t,
    tm,
    setLanguage,
    translateTheme,
    getThemeTranslationKey,
  }
}

/**
 * Changes the persisted language preference and loads its dictionary.
 *
 * @param language Language code, or `auto`/`null` for browser-based detection.
 */
export async function setLanguage(language: string | null): Promise<void> {
  const normalizedLanguage = normalizeLanguagePreference(language)
  const storage = useStorage()
  storage.getParameters().language.value = normalizedLanguage
  selectedLanguage.value = normalizedLanguage
  await applyActiveLanguage(resolveLanguage(normalizedLanguage))
}

/**
 * Translates one message key with optional placeholder replacement.
 *
 * @param key Dot-separated translation key.
 * @param params Optional placeholder values.
 * @returns Translated text, falling back to English or the key.
 */
export function t(key: string, params: TranslationParams = {}): string {
  const activeValue = readMessage(activeMessages.value, key)
  const fallbackValue = readMessage(fallbackMessages.value, key)
  const message = typeof activeValue === 'string'
    ? activeValue
    : typeof fallbackValue === 'string'
      ? fallbackValue
      : key

  return formatMessage(message, params)
}

/**
 * Returns a translated string map for grouped labels.
 *
 * @param key Dot-separated translation map key.
 * @returns Translation map, or an empty object when unavailable.
 */
export function tm(key: string): Record<string, string> {
  const activeValue = readMessage(activeMessages.value, key)
  const fallbackValue = readMessage(fallbackMessages.value, key)

  if (isStringRecord(activeValue)) {
    return activeValue
  }

  return isStringRecord(fallbackValue) ? fallbackValue : {}
}

/**
 * Translates a backend theme value while keeping the value itself canonical.
 *
 * @param value Backend theme value.
 * @returns Localized theme label, or the backend value when untranslated.
 */
export function translateTheme(value: string): string {
  const key = getThemeTranslationKey(value)
  return tm('themes')[key] ?? value
}

/**
 * Builds the stable translation key used for a backend theme value.
 *
 * @param value Backend theme value.
 * @returns Normalized translation key.
 */
export function getThemeTranslationKey(value: string): string {
  return value
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
}

/**
 * Loads the locale index, fallback dictionary, and active dictionary.
 *
 * @returns Promise resolved once all locale data is loaded.
 */
async function loadInitialI18n(): Promise<void> {
  const storage = useStorage()
  selectedLanguage.value = normalizeLanguagePreference(storage.getParameters().language.value)

  isLoading.value = true
  errorMessage.value = null

  try {
    const index = await loadLocaleIndex()
    defaultLanguage.value = index.defaultLanguage || DEFAULT_LANGUAGE
    availableLanguages.value = index.languages.length
      ? index.languages
      : [{ code: defaultLanguage.value, label: defaultLanguage.value }]

    fallbackMessages.value = await loadDictionary(defaultLanguage.value)
    await applyActiveLanguage(resolveLanguage(selectedLanguage.value))
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : 'Unable to load translations.'
    fallbackMessages.value = {}
    activeMessages.value = {}
    resolvedLanguage.value = defaultLanguage.value
  } finally {
    isLoading.value = false
  }

  watch(
    storage.getParameters().language,
    (nextLanguage) => {
      const normalizedLanguage = normalizeLanguagePreference(nextLanguage)
      selectedLanguage.value = normalizedLanguage
      void applyActiveLanguage(resolveLanguage(normalizedLanguage))
    },
  )
}

/**
 * Applies one concrete language as the active dictionary.
 *
 * @param language Concrete language code.
 * @returns Promise resolved once the language is applied.
 */
async function applyActiveLanguage(language: string): Promise<void> {
  resolvedLanguage.value = language

  if (language === defaultLanguage.value) {
    activeMessages.value = fallbackMessages.value
    return
  }

  try {
    activeMessages.value = await loadDictionary(language)
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : 'Unable to load translations.'
    activeMessages.value = fallbackMessages.value
    resolvedLanguage.value = defaultLanguage.value
  }
}

/**
 * Loads the language index from public locale assets.
 *
 * @returns Available locale metadata.
 */
async function loadLocaleIndex(): Promise<LocaleIndex> {
  const response = await fetch(`${LOCALE_BASE_PATH}/index.json`)
  if (!response.ok) {
    throw new Error(`Unable to load locale index (${response.status}).`)
  }

  const payload: unknown = await response.json()
  if (!isLocaleIndex(payload)) {
    throw new Error('Invalid locale index format.')
  }

  return payload
}

/**
 * Loads a translation dictionary from public locale assets.
 *
 * @param language Concrete language code.
 * @returns Translation dictionary.
 */
async function loadDictionary(language: string): Promise<TranslationDictionary> {
  const response = await fetch(`${LOCALE_BASE_PATH}/${encodeURIComponent(language)}.json`)
  if (!response.ok) {
    throw new Error(`Unable to load ${language} translations (${response.status}).`)
  }

  const payload: unknown = await response.json()
  return isDictionary(payload) ? payload : {}
}

/**
 * Resolves Auto/browser selection to an available concrete language.
 *
 * @param language Persisted language preference.
 * @returns Available language code.
 */
function resolveLanguage(language: string | null): string {
  if (language && hasLanguage(language)) {
    return language
  }

  const browserLanguages = typeof navigator === 'undefined'
    ? []
    : [...(navigator.languages ?? []), navigator.language].filter(Boolean)

  for (const browserLanguage of browserLanguages) {
    const normalizedBrowserLanguage = browserLanguage.toLowerCase()
    const exactMatch = availableLanguages.value.find(
      (candidate) => candidate.code.toLowerCase() === normalizedBrowserLanguage,
    )
    if (exactMatch) {
      return exactMatch.code
    }

    const baseLanguage = normalizedBrowserLanguage.split('-')[0]
    const baseMatch = availableLanguages.value.find(
      (candidate) => candidate.code.toLowerCase() === baseLanguage,
    )
    if (baseMatch) {
      return baseMatch.code
    }
  }

  return hasLanguage(defaultLanguage.value) ? defaultLanguage.value : DEFAULT_LANGUAGE
}

/**
 * Checks if a concrete language exists in the loaded index.
 *
 * @param language Language code to check.
 * @returns True when the language exists.
 */
function hasLanguage(language: string): boolean {
  return availableLanguages.value.some(
    (candidate) => candidate.code.toLowerCase() === language.toLowerCase(),
  )
}

/**
 * Normalizes persisted language values.
 *
 * @param language Raw language preference.
 * @returns Concrete code, or `null` for Auto.
 */
function normalizeLanguagePreference(language: string | null | undefined): string | null {
  const trimmedLanguage = language?.trim()
  if (!trimmedLanguage || trimmedLanguage.toLowerCase() === AUTO_LANGUAGE) {
    return null
  }

  return trimmedLanguage
}

/**
 * Reads one dot-separated value from a dictionary.
 *
 * @param dictionary Translation dictionary.
 * @param key Dot-separated key.
 * @returns Matching translation node.
 */
function readMessage(dictionary: TranslationDictionary, key: string): unknown {
  return key.split('.').reduce<unknown>((current, part) => {
    if (!isRecord(current)) {
      return undefined
    }

    return current[part]
  }, dictionary)
}

/**
 * Replaces `{name}` placeholders in translated messages.
 *
 * @param message Message template.
 * @param params Placeholder values.
 * @returns Formatted message.
 */
function formatMessage(message: string, params: TranslationParams): string {
  return message.replace(/\{([^}]+)}/g, (match, key) => {
    const value = params[key]
    return value === null || value === undefined ? match : String(value)
  })
}

/**
 * Checks if a value is an object record.
 *
 * @param value Value to check.
 * @returns True when the value is a non-array record.
 */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/**
 * Checks if a value is a string map.
 *
 * @param value Value to check.
 * @returns True when every value is a string.
 */
function isStringRecord(value: unknown): value is Record<string, string> {
  return isRecord(value) && Object.values(value).every((entry) => typeof entry === 'string')
}

/**
 * Checks if a value is a translation dictionary.
 *
 * @param value Value to check.
 * @returns True when the value can be used as a dictionary.
 */
function isDictionary(value: unknown): value is TranslationDictionary {
  return isRecord(value)
}

/**
 * Checks if a value has the locale index shape.
 *
 * @param value Value to check.
 * @returns True when the value is a locale index.
 */
function isLocaleIndex(value: unknown): value is LocaleIndex {
  if (!isRecord(value) || typeof value.defaultLanguage !== 'string' || !Array.isArray(value.languages)) {
    return false
  }

  return value.languages.every(
    (language) =>
      isRecord(language) &&
      typeof language.code === 'string' &&
      typeof language.label === 'string',
  )
}
