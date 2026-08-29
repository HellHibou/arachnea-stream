import { readonly, shallowRef } from 'vue'

import { resolveAppPath } from '@/services/baseUrl'
import type {
  LocaleIndex,
  LocaleIndexLanguage,
  TranslationDictionary,
  TranslationParams,
} from './types'

/** Default fallback language code. */
const DEFAULT_LANGUAGE = 'en'
/** Base path for locale JSON files. */
const LOCALE_BASE_PATH = resolveAppPath('locales')

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
}

async function resolveActiveLanguage(): Promise<void> {
  const targetLanguage = selectedLanguage.value ?? detectBrowserLanguage()
  resolvedLanguage.value = targetLanguage
  await loadDictionary(targetLanguage, activeMessages)
  await loadFallbackDictionary()
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
  try {
    const response = await fetch(`${LOCALE_BASE_PATH}/${DEFAULT_LANGUAGE}.json`)
    if (!response.ok) {
      fallbackMessages.value = {}
      return
    }
    const payload: unknown = await response.json()
    if (isDictionary(payload)) {
      fallbackMessages.value = payload
    }
  } catch {
    fallbackMessages.value = {}
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

