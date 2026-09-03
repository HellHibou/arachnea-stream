import { readonly, shallowRef } from 'vue'

import { resolveAppPath } from '@/services/baseUrl'
import localeEn from '@/../public/locales/en.json'
import localeFr from '@/../public/locales/fr.json'
import type {
  LocaleIndex,
  LocaleIndexLanguage,
  TranslationDictionary,
  TranslationParams,
} from './types'

/** Default fallback language code. */
const DEFAULT_LANGUAGE = 'en'
/** Base path for locale JSON files (used only for index.json). */
const LOCALE_BASE_PATH = resolveAppPath('locales')

/** Bundled dictionaries (imported statically to avoid base-path fetch issues). */
const BUNDLED_LOCALES: Record<string, TranslationDictionary> = {
  en: localeEn as TranslationDictionary,
  fr: localeFr as TranslationDictionary,
}

/** List of languages available for selection. */
const availableLanguages = shallowRef<LocaleIndexLanguage[]>([
  { code: DEFAULT_LANGUAGE, flag: '🇬🇧', labelLocal: 'English', labelEn: 'English' },
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
/** Error message from the last translation loading attempt. */
const errorMessage = shallowRef<string | null>(null)
/** Promise for tracking i18n initialization. */
let initPromise: Promise<void> | null = null

/** Special language code for automatic browser language detection. */
export const AUTO_LANGUAGE = 'auto'

export function initializeI18n(): Promise<void> {
  if (initPromise) {
    return initPromise
  }
  initPromise = loadInitialI18n()
  return initPromise
}

export function useI18n() {
  return {
    availableLanguages: readonly(availableLanguages),
    selectedLanguage: readonly(selectedLanguage),
    resolvedLanguage: readonly(resolvedLanguage),
    isLoading: readonly(isLoading),
    errorMessage: readonly(errorMessage),
    t,
    serviceStoreTitle,
    serviceStoreDescription,
    setLanguage,
  }
}

export async function setLanguage(language: string | null): Promise<void> {
  const normalizedLanguage = normalizeLanguagePreference(language)
  selectedLanguage.value = normalizedLanguage
  await resolveActiveLanguage()
}

export function t(key: string, params?: TranslationParams): string {
  const activeValue = readMessage(activeMessages.value, key)
  if (typeof activeValue === 'string') {
    return formatMessage(activeValue, params ?? {})
  }
  const fallbackValue = readMessage(fallbackMessages.value, key)
  if (typeof fallbackValue === 'string') {
    return formatMessage(fallbackValue, params ?? {})
  }
  return key
}

/**
 * Resolves the localized title of one service group.
 *
 * Fallback chain: selected locale, then English, then the technical group
 * identifier. The result is reactive and recomputes when the active language
 * changes.
 *
 * @param storeId - Technical `service_store_id` of the group.
 * @returns The localized title, or the group identifier when absent.
 */
export function serviceStoreTitle(storeId: string): string {
  return resolveServiceStoreTexts(storeId).title
}

/**
 * Resolves the localized description of one service group.
 *
 * Fallback chain: selected locale, then English, then no description. The
 * result is reactive and recomputes when the active language changes.
 *
 * @param storeId - Technical `service_store_id` of the group.
 * @returns The localized description, or `undefined` when absent.
 */
export function serviceStoreDescription(storeId: string): string | undefined {
  return resolveServiceStoreTexts(storeId).description
}

/**
 * Reads the group title/description across the active and fallback
 * dictionaries. A missing, non-textual or empty value is treated as absent.
 */
function resolveServiceStoreTexts(
  storeId: string,
): { title: string; description: string | undefined } {
  let title: string | undefined
  let description: string | undefined
  for (const dictionary of [activeMessages.value, fallbackMessages.value]) {
    const storeTexts = readMessage(dictionary, `service-store.${storeId}`)
    if (!isRecord(storeTexts)) {
      continue
    }
    const rawTitle = storeTexts.title
    const rawDescription = storeTexts.description
    if (title === undefined && typeof rawTitle === 'string' && rawTitle.trim().length > 0) {
      title = rawTitle
    }
    if (
      description === undefined
      && typeof rawDescription === 'string'
      && rawDescription.trim().length > 0
    ) {
      description = rawDescription
    }
  }
  return { title: title ?? storeId, description }
}

async function loadInitialI18n(): Promise<void> {
  isLoading.value = true
  errorMessage.value = null
  try {
    await loadLocaleIndex()
    await resolveActiveLanguage()
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error)
    await loadFallbackDictionary()
  } finally {
    isLoading.value = false
  }
}

async function loadLocaleIndex(): Promise<void> {
  const response = await fetch(resolveAppPath('locales/index.json'))
  if (!response.ok) {
    throw new Error(`Unable to load locale index (${response.status}).`)
  }
  const payload: unknown = await response.json()
  if (!isLocaleIndex(payload)) {
    throw new Error('Invalid locale index format.')
  }
  defaultLanguage.value = payload.defaultLanguage
  availableLanguages.value = payload.languages
  await loadFallbackDictionary()
}

async function resolveActiveLanguage(): Promise<void> {
  const targetLanguage = selectedLanguage.value ?? detectBrowserLanguage()
  resolvedLanguage.value = targetLanguage
  await loadDictionary(targetLanguage, activeMessages)
}

async function loadDictionary(
  language: string,
  target: typeof activeMessages,
): Promise<void> {
  if (language === DEFAULT_LANGUAGE) {
    target.value = fallbackMessages.value
    return
  }
  try {
    const response = await fetch(`${LOCALE_BASE_PATH}/${language}.json`)
    if (!response.ok) {
      throw new Error(`Unable to load ${language} dictionary (${response.status}).`)
    }
    const payload: unknown = await response.json()
    if (!isDictionary(payload)) {
      throw new Error(`Invalid ${language} dictionary format.`)
    }
    target.value = payload
  } catch {
    target.value = {}
  }
}

async function loadFallbackDictionary(): Promise<void> {
  if (Object.keys(fallbackMessages.value).length > 0) {
    return
  }
  // Use bundled English dictionary as fallback (avoids fetch under base path).
  if (BUNDLED_LOCALES[DEFAULT_LANGUAGE]) {
    fallbackMessages.value = BUNDLED_LOCALES[DEFAULT_LANGUAGE]!
  }
}

function detectBrowserLanguage(): string {
  const navigatorLanguages = navigator.languages ?? [navigator.language]
  for (const rawLanguage of navigatorLanguages) {
    const baseLanguage = rawLanguage.split('-')[0]?.toLowerCase()
    if (!baseLanguage) {
      continue
    }
    const baseMatch = availableLanguages.value.find(
      (candidate) => candidate.code.toLowerCase() === baseLanguage,
    )
    if (baseMatch) {
      return baseMatch.code
    }
  }
  return hasLanguage(defaultLanguage.value) ? defaultLanguage.value : DEFAULT_LANGUAGE
}

function hasLanguage(language: string): boolean {
  return availableLanguages.value.some(
    (candidate) => candidate.code.toLowerCase() === language.toLowerCase(),
  )
}

function normalizeLanguagePreference(language: string | null | undefined): string | null {
  const trimmedLanguage = language?.trim()
  if (!trimmedLanguage || trimmedLanguage.toLowerCase() === AUTO_LANGUAGE) {
    return null
  }
  return trimmedLanguage
}

function readMessage(dictionary: TranslationDictionary, key: string): unknown {
  return key.split('.').reduce<unknown>((current, part) => {
    if (!isRecord(current)) {
      return undefined
    }
    return current[part]
  }, dictionary)
}

function formatMessage(message: string, params: TranslationParams): string {
  return message.replace(/\{([^}]+)\}/g, (_match, key) => {
    const value = params[key]
    return value === null || value === undefined ? _match : String(value)
  })
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isDictionary(value: unknown): value is TranslationDictionary {
  return isRecord(value)
}

function isLocaleIndex(value: unknown): value is LocaleIndex {
  if (!isRecord(value) || typeof value.defaultLanguage !== 'string' || !Array.isArray(value.languages)) {
    return false
  }
  return value.languages.every(
    (language) =>
      isRecord(language) &&
      typeof language.code === 'string' &&
      typeof language.flag === 'string' &&
      typeof language.labelLocal === 'string' &&
      typeof language.labelEn === 'string',
  )
}