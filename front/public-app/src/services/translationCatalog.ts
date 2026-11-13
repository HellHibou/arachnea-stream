import { shallowReadonly, shallowRef } from 'vue'
import { getBaseLanguageCode, normalizeLanguageCode } from '@/services/languages'
import { getTranslationServices } from '@/services/rustify'
import type { TranslationCatalog, TranslationServiceCapabilities } from '@/types/translation'

const catalog = shallowRef<TranslationCatalog | null>(null)
let pending: Promise<TranslationCatalog | null> | null = null

/** Shared, read-only effective catalog for settings and subtitle translation. */
export const translationCatalog = shallowReadonly(catalog)

/**
 * Loads the catalog once, sharing concurrent requests and allowing retries after failure.
 * @returns The effective catalog, or null when loading failed.
 */
export function ensureTranslationCatalog(): Promise<TranslationCatalog | null> {
  if (catalog.value) return Promise.resolve(catalog.value)
  if (!pending) {
    pending = getTranslationServices()
      .then((loaded) => {
        catalog.value = loaded
        return loaded
      })
      .catch(() => null)
      .finally(() => { pending = null })
  }
  return pending
}

/**
 * Converts a subtitle language to the two-letter translation contract.
 * @param value Raw subtitle language.
 * @returns Two-letter language code, or null.
 */
export function toTranslationCode(value: unknown): string | null {
  const base = getBaseLanguageCode(normalizeLanguageCode(value))
  return base && /^[a-z]{2}$/.test(base) ? base : null
}

/**
 * Checks whether a service supports WebVTT translation for a particular source and target.
 * @param service Effective service capabilities.
 * @param source Source code, or null if detection is required.
 * @param target Target language code.
 * @param requiresText Whether the source requires text-based translation.
 * @returns Whether this service can be selected.
 */
export function serviceSupportsSubtitlePair(
  service: TranslationServiceCapabilities,
  source: string | null,
  target: string,
  requiresText = false,
): boolean {
  const supportsUrl = service.supportsUrlTranslation
    && service.urlTranslationFormats.some((format) => format.toLowerCase() === 'text/vtt')
  if (!service.supportsTextTranslation && (requiresText || !supportsUrl)) return false
  if (!service.targetLanguages.includes(target) || source === target) return false
  if (source ? !service.sourceLanguages.includes(source) : !service.sourceDetection) return false
  if (!service.supportedPairs) return true
  if (!source && !service.sourceDetectionIdentifiesLanguage) return false
  return service.supportedPairs.some((pair) => pair.target === target && (!source || pair.source === source))
}

/**
 * Checks whether any usable direction reaches a WebVTT translation target.
 * @param service Effective service capabilities.
 * @param target Two-letter target language.
 * @returns Whether the target can be generated from at least one source.
 */
export function serviceSupportsSubtitleTarget(service: TranslationServiceCapabilities, target: string): boolean {
  return service.sourceLanguages.some((source) => serviceSupportsSubtitlePair(service, source, target))
}
