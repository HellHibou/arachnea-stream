import { t } from '@/i18n'
import type { ServiceMetadata, ServiceThemeMetadata } from '@/types/serviceMetadata'

import { call_api } from '../apiClient'
import {
  firstNonEmptyString,
  isJsonRecord,
  readRecordList,
  readStringMap,
} from '../primitives/readers'

/**
 * Loads display metadata for all configured backend services.
 *
 * @returns Normalized service metadata records indexed by their `id` field.
 */
export async function loadServiceMetadata(): Promise<ServiceMetadata[]> {
  const response = await call_api<unknown>('get_service', {})

  if (!Array.isArray(response)) {
    throw new Error(t('errors.unexpectedMetadataResponse'))
  }

  return response
    .map(normalizeServiceMetadata)
    .filter((metadata): metadata is ServiceMetadata => Boolean(metadata))
}

/**
 * Converts one backend service row into frontend display metadata.
 *
 * @param payload Raw backend row returned by `get_service`.
 * @returns Normalized service metadata, or `null` when the row does not expose an id.
 */
export function normalizeServiceMetadata(payload: unknown): ServiceMetadata | null {
  const record = isJsonRecord(payload) ? payload : {}
  const id = firstNonEmptyString([record.id])

  if (!id) {
    return null
  }

  return {
    id,
    title: firstNonEmptyString([record.title]) ?? id,
    logo: firstNonEmptyString([record.logo]),
    description: normalizeServiceDescription(record.description),
    metadata: isJsonRecord(record.metadata)
      ? Object.fromEntries(Object.entries(record.metadata).filter(([, value]) => Array.isArray(value))
          .map(([key, value]) => [key, (value as unknown[]).filter((item): item is string => typeof item === 'string')]))
      : {},
    themes: normalizeServiceThemes(record.themes ?? record.search_themes),
    supportedQueries: Array.isArray(record.supported_queries)
      ? record.supported_queries.filter((query): query is string => typeof query === 'string')
      : [],
  }
}

/**
 * Reads localized service descriptions from the typed backend object.
 *
 * @param value Backend description node.
 * @returns Language-keyed description strings.
 */
function normalizeServiceDescription(value: unknown): Record<string, string> {
  if (isJsonRecord(value) && !Array.isArray(value)) {
    return readStringMap(value)
  }

  return {}
}

/**
 * Converts backend theme mapping rows into frontend service theme metadata.
 *
 * @param value Backend themes node.
 * @returns Theme mappings declared by the service.
 */
function normalizeServiceThemes(value: unknown): ServiceThemeMetadata[] {
  if (Array.isArray(value) && value.every((theme) => typeof theme === 'string')) {
    return value
      .map((theme) => theme.trim())
      .filter((theme) => theme.length > 0)
      .map((theme) => ({ code: theme, serviceCode: theme }))
  }

  return readRecordList(value)
    .map((theme) => {
      const code = firstNonEmptyString([theme.code])
      const serviceCode = firstNonEmptyString([theme.service_code, theme.serviceCode])

      if (!code || !serviceCode) {
        return null
      }

      return { code, serviceCode }
    })
    .filter((theme): theme is ServiceThemeMetadata => Boolean(theme))
}
