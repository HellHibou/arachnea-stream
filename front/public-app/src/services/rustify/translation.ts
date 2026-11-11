import { t } from '@/i18n'
import type {
  PreparedSubtitleTranslation,
  SubtitleTranslationLanguageRequest,
  SubtitleTranslationPolicy,
  TranslationCatalog,
  TranslationLanguagePair,
  TranslationServiceCapabilities,
} from '@/types/translation'

import { call_api } from './apiClient'
import {
  firstNonEmptyString,
  isJsonRecord,
  normalizeString,
  readOptionalBoolean,
  readStringList,
} from './primitives/readers'

/**
 * Loads the public effective translation catalog.
 *
 * @returns Enabled services, their capabilities and the reachable languages.
 */
export async function getTranslationServices(): Promise<TranslationCatalog> {
  const response = await call_api<unknown>('get_translation_services', {})

  const catalog = normalizeTranslationCatalog(response)
  if (!catalog) {
    throw new Error(t('errors.unexpectedTranslationCatalogResponse'))
  }

  return catalog
}

/**
 * Prepares one subtitle translation descriptor without retrieving or translating.
 *
 * @param request Language request carrying the authorized source track.
 * @returns Track-shaped link and metadata accepted by the player contract.
 */
export async function prepareSubtitleTranslation(
  request: SubtitleTranslationLanguageRequest,
): Promise<PreparedSubtitleTranslation> {
  const targetLanguage = normalizeTranslationLanguage(request.targetLanguage)
  if (!targetLanguage) {
    throw new Error('Invalid subtitle translation target language.')
  }
  const sourceLanguage = normalizeTranslationLanguage(request.sourceLanguage)
  const trackId = normalizeOptionalText(request.source.trackId)
  const response = await call_api<unknown>('prepare_subtitle_translation', {
    url: request.source.url,
    ...(trackId ? { track_id: trackId } : {}),
    ...(sourceLanguage ? { source_language: sourceLanguage } : {}),
    target_language: targetLanguage,
    ...(request.policy ? { policy: toBackendPolicy(request.policy) } : {}),
    ...(request.source.format ? { format: request.source.format } : {}),
  })

  const prepared = normalizePreparedSubtitleTranslation(response)
  if (!prepared) {
    throw new Error(t('errors.unexpectedSubtitleTranslationResponse'))
  }

  return prepared
}

/**
 * Normalizes the public catalog returned by `get_translation_services`.
 *
 * @param payload Raw backend payload.
 * @returns Normalized catalog, or `null` when the payload is unusable.
 */
export function normalizeTranslationCatalog(payload: unknown): TranslationCatalog | null {
  if (!isJsonRecord(payload) || !Array.isArray(payload.services)) {
    return null
  }

  const services = payload.services
    .map(normalizeTranslationService)
    .filter((service): service is TranslationServiceCapabilities => Boolean(service))

  return {
    services,
    languages: readNormalizedLanguages(payload.languages),
  }
}

/**
 * Normalizes one public translation service row.
 *
 * @param payload Raw service row.
 * @returns Normalized capabilities, or `null` when the service identifier is missing.
 */
function normalizeTranslationService(payload: unknown): TranslationServiceCapabilities | null {
  if (!isJsonRecord(payload)) {
    return null
  }

  const serviceId = firstNonEmptyString([payload.service_id, payload.serviceId])
  if (!serviceId) {
    return null
  }

  return {
    serviceId,
    title: firstNonEmptyString([payload.title]) ?? serviceId,
    languages: readNormalizedLanguages(payload.languages),
    sourceLanguages: readNormalizedLanguages(payload.source_languages ?? payload.sourceLanguages),
    targetLanguages: readNormalizedLanguages(payload.target_languages ?? payload.targetLanguages),
    supportedPairs: normalizeSupportedPairs(
      payload.supported_pairs ?? payload.supportedPairs,
    ),
    supportsTextTranslation: readOptionalBoolean(
      payload.supports_text_translation ?? payload.supportsTextTranslation,
    ) === true,
    supportsUrlTranslation: readOptionalBoolean(
      payload.supports_url_translation ?? payload.supportsUrlTranslation,
    ) === true,
    sourceDetection: readOptionalBoolean(
      payload.source_detection ?? payload.sourceDetection,
    ) === true,
    preferred: readOptionalBoolean(payload.preferred) === true,
  }
}

/**
 * Reads a language list with stable de-duplication.
 *
 * @param value Raw backend language list.
 * @returns Lowercase non-empty language codes in declaration order.
 */
function readNormalizedLanguages(value: unknown): string[] {
  const languages: string[] = []
  const seen = new Set<string>()

  for (const language of readStringList(value)) {
    const normalized = language.toLocaleLowerCase()
    if (!seen.has(normalized)) {
      seen.add(normalized)
      languages.push(normalized)
    }
  }

  return languages
}

/**
 * Normalizes the optional directional pair restriction.
 *
 * @param value Raw `supported_pairs` value.
 * @returns Pair list, or `null` when the service did not restrict directions.
 */
function normalizeSupportedPairs(value: unknown): TranslationLanguagePair[] | null {
  if (value === undefined || value === null) {
    return null
  }

  if (!Array.isArray(value)) {
    return []
  }

  return value.flatMap((item) => {
    if (!isJsonRecord(item)) {
      return []
    }

    const source = firstNonEmptyString([item.source])?.toLocaleLowerCase()
    const target = firstNonEmptyString([item.target])?.toLocaleLowerCase()
    return source && target ? [{ source, target }] : []
  })
}

/**
 * Normalizes one preparation response into the existing player track contract.
 *
 * @param payload Raw backend payload.
 * @returns Prepared track metadata, or `null` when the required fields are missing.
 */
function normalizePreparedSubtitleTranslation(payload: unknown): PreparedSubtitleTranslation | null {
  if (!isJsonRecord(payload)) {
    return null
  }

  const link = firstNonEmptyString([payload.link])
  const lang = firstNonEmptyString([payload.lang])
  if (!link || !lang) {
    return null
  }

  return {
    link,
    lang,
    label: firstNonEmptyString([payload.label]) ?? lang.toLocaleUpperCase(),
    autoTranslate: true,
  }
}

/**
 * Converts the frontend policy model into the backend wire shape.
 *
 * @param policy Automatic or constrained selection policy.
 * @returns Backend-compatible policy object.
 */
function toBackendPolicy(policy: SubtitleTranslationPolicy): Record<string, unknown> {
  if (policy.mode === 'constrained') {
    return { mode: 'constrained', service_id: policy.serviceId }
  }

  const preferredService = normalizeOptionalText(policy.preferredService)
  return {
    mode: 'auto',
    ...(preferredService ? { preferred_service: preferredService } : {}),
  }
}

/**
 * Trims an optional string option.
 *
 * @param value Raw optional value.
 * @returns Trimmed string, or `null` when empty.
 */
function normalizeOptionalText(value: unknown): string | null {
  return typeof value === 'string' ? normalizeString(value) : null
}

/**
 * Normalizes one optional language code for the two-letter backend contract.
 *
 * @param value Raw optional language code.
 * @returns Two-letter lowercase code, or `null` when missing or unsupported.
 */
function normalizeTranslationLanguage(value: unknown): string | null {
  const normalized = normalizeOptionalText(value)
  if (!normalized) {
    return null
  }

  const code = normalized.toLocaleLowerCase()
  return /^[a-z]{2}$/.test(code) ? code : null
}
