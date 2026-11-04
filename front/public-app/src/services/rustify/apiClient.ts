import { t } from '@/i18n'
import { resolveAppPath } from '@/services/baseUrl'
import { enqueueErrorNotification } from '@/composables/useErrorNotifications'
import type {
  ScraperAggregationResult,
  ScraperExecutionError,
} from '@/types/scraperError'

import { firstNonEmptyString, isJsonRecord, normalizeString } from './primitives/readers'
import type { TauriWindow } from './desktop'

/** Base URL for the REST API. Falls back to the `api` segment under the app base when not configured via environment. */
const restApiBaseUrl = import.meta.env.VITE_RUSTIFY_API_BASE_URL ?? resolveAppPath('api')

/** Cached conditional-validation entry keyed by command name and serialized parameters. */
interface EtagCacheEntry {
  /** Global ETag returned by the backend for this request. */
  etag: string
  /** Raw JSON envelope previously returned for this request. */
  payload: unknown
}

/** In-memory conditional-validation cache shared by every REST GET call. */
const etagCache = new Map<string, EtagCacheEntry>()

/**
 * Builds the cache key of one REST GET request from its command and parameters.
 *
 * @param fct_name Backend function name.
 * @param queryString Serialized query string of the request.
 * @returns Stable cache key scoped to this exact request.
 */
function buildEtagCacheKey(fct_name: string, queryString: string): string {
  return `${fct_name}?${queryString}`
}

/**
 * Calls a backend function through either Tauri or the REST bridge.
 *
 * The REST path issues a GET request whose parameters are JSON-encoded into
 * the query string, and participates in conditional validation: the global
 * ETag returned by the backend is cached per request shape and replayed as
 * `If-None-Match` and `arachneaEtag`. The latter lets Scrapyfy validate the
 * root response before it executes dependent sub-queries; a `304 Not Modified`
 * answer resolves to the cached payload.
 * The Tauri path keeps its original invoke semantics.
 *
 * @param fct_name Backend function name.
 * @param params Function parameters.
 * @returns Deserialized backend payload.
 */
export async function call_api<T = unknown>(
  fct_name: string,
  params: Record<string, unknown>,
): Promise<T> {
  const tauriWindow = window as TauriWindow
  const tauriInvoke =
    tauriWindow.__TAURI__?.core?.invoke ?? tauriWindow.__TAURI__?.invoke

  try {
    if (tauriInvoke) {
      const payload = await tauriInvoke<unknown>(fct_name, params)
      return unwrapAggregationPayload<T>(payload, fct_name)
    }

    // Serialize every parameter as JSON so complex values (arrays, objects,
    // quoted strings) survive the query-string round trip; the backend
    // JSON-decodes values that start with `[`, `{`, or `"`.
    const searchParams = new URLSearchParams()
    for (const [key, value] of Object.entries(params)) {
      if (value === undefined) {
        continue
      }
      searchParams.append(key, JSON.stringify(value))
    }
    const queryString = searchParams.toString()
    const cacheKey = buildEtagCacheKey(fct_name, queryString)
    const cachedEntry = etagCache.get(cacheKey)

    const headers: Record<string, string> = {}
    if (cachedEntry) {
      headers['If-None-Match'] = `"${cachedEntry.etag}"`
      // Keep the validator out of cacheKey, otherwise every refresh would use
      // a different cache entry. The backend uses this value to decode the
      // source fragment before scheduling scraper sub-queries.
      searchParams.append('arachneaEtag', JSON.stringify(cachedEntry.etag))
    }
    const requestQueryString = searchParams.toString()

    const response = await fetch(
      `${restApiBaseUrl}/${fct_name}${requestQueryString ? `?${requestQueryString}` : ''}`,
      { method: 'GET', headers },
    )

    if (response.status === 304 && cachedEntry) {
      return unwrapAggregationPayload<T>(cachedEntry.payload, fct_name)
    }

    const payload = await parseResponseBody(response)

    if (!response.ok) {
      handleFailedRestResponse(payload, fct_name, response.status)
    }

    const etagHeader = response.headers.get('ETag')
    if (etagHeader) {
      etagCache.set(cacheKey, {
        etag: etagHeader.replace(/^W?"/, '').replace(/"$/, ''),
        payload,
      })
    } else {
      etagCache.delete(cacheKey)
    }

    return unwrapAggregationPayload<T>(payload, fct_name)
  } catch (error) {
    if (error instanceof ReportedApiError) {
      throw error
    }

    const message = error instanceof Error ? error.message : String(error)
    throwFrontendApiError(fct_name, message)
  }
}

/**
 * Validates a JSON command envelope, emits its errors, and returns its data.
 * Bare payloads remain accepted temporarily during the backend rollout.
 */
function unwrapAggregationPayload<T>(payload: unknown, operation: string): T {
  const aggregationPayload = readAggregationPayload<T>(payload)

  if (aggregationPayload.kind === 'malformed') {
    return throwFrontendApiError(operation, 'Malformed scraper aggregation response envelope.')
  }

  if (aggregationPayload.kind === 'bare') {
    return payload as T
  }

  reportExecutionErrors(aggregationPayload.value.errors)
  return aggregationPayload.value.data
}

/** Handles REST failures while preserving structured backend errors when available. */
function handleFailedRestResponse(payload: unknown, operation: string, status: number): never {
  const aggregationPayload = readAggregationPayload<unknown>(payload)

  if (aggregationPayload.kind === 'envelope') {
    const firstError = aggregationPayload.value.errors[0]
    if (firstError) {
      reportExecutionErrors(aggregationPayload.value.errors)
      throw new ReportedApiError(firstError.message)
    }
  }

  const message = extractErrorMessage(payload) ??
    t('errors.restCallFailed', { function: operation, status })
  return throwFrontendApiError(operation, message)
}

/** Distinguishes valid envelopes, transitional bare payloads, and malformed envelopes. */
function readAggregationPayload<T>(payload: unknown): AggregationPayload<T> {
  if (!isJsonRecord(payload) || (!('data' in payload) && !('errors' in payload))) {
    return { kind: 'bare' }
  }

  if (!('data' in payload) || !Array.isArray(payload.errors)) {
    return { kind: 'malformed' }
  }

  if (!payload.errors.every(isScraperExecutionError)) {
    return { kind: 'malformed' }
  }

  return {
    kind: 'envelope',
    value: {
      data: payload.data as T,
      errors: payload.errors,
    },
  }
}

/** Returns whether a JSON value is a complete structured execution error. */
function isScraperExecutionError(value: unknown): value is ScraperExecutionError {
  if (!isJsonRecord(value)) {
    return false
  }

  return typeof value.code === 'string' &&
    typeof value.operation === 'string' &&
    (typeof value.source === 'string' || value.source === null) &&
    (value.origin === 'backend' || value.origin === 'frontend') &&
    typeof value.message === 'string'
}

/** Logs and queues every structured execution error. */
function reportExecutionErrors(errors: ScraperExecutionError[]): void {
  errors.forEach((error) => {
    console.error('Scraper API error', {
      code: error.code,
      operation: error.operation,
      source: error.source,
      origin: error.origin,
      message: error.message,
    })
    enqueueErrorNotification(error)
  })
}

/** Creates, logs, queues, and rejects a frontend-generated technical error. */
function throwFrontendApiError(operation: string, message: string): never {
  const error: ScraperExecutionError = {
    code: nextFrontendErrorCode(),
    operation,
    source: null,
    origin: 'frontend',
    message,
  }
  reportExecutionErrors([error])
  throw new ReportedApiError(message)
}

let lastFrontendErrorMillis = 0
let frontendErrorSequence = 0

/** Generates a monotonic browser-local diagnostic code matching the backend format. */
function nextFrontendErrorCode(): string {
  const currentMillis = Date.now()

  if (currentMillis <= lastFrontendErrorMillis) {
    frontendErrorSequence += 1
    if (frontendErrorSequence > 99) {
      lastFrontendErrorMillis += 1
      frontendErrorSequence = 0
    }
  } else {
    lastFrontendErrorMillis = currentMillis
    frontendErrorSequence = 0
  }

  return `ARACHNEA_E${lastFrontendErrorMillis}${String(frontendErrorSequence).padStart(2, '0')}`
}

/** Signals that a structured error has already been queued and logged. */
class ReportedApiError extends Error {}

type AggregationPayload<T> =
  | { kind: 'envelope', value: ScraperAggregationResult<T> }
  | { kind: 'bare' }
  | { kind: 'malformed' }

/**
 * Parses a REST response body while tolerating empty payloads.
 *
 * @param response Fetch response returned by the REST bridge.
 * @returns Parsed JSON payload or `null` when the body is empty.
 */
async function parseResponseBody(response: Response): Promise<unknown> {
  const body = await response.text()

  if (!body) {
    throw new Error('Empty JSON response body.')
  }

  try {
    return JSON.parse(body)
  } catch {
    throw new Error('Invalid JSON response body.')
  }
}

/**
 * Extracts a displayable message from an unknown backend error payload.
 *
 * @param payload Backend error payload.
 * @returns Human-readable error text when available.
 */
function extractErrorMessage(payload: unknown): string | null {
  if (typeof payload === 'string') {
    return normalizeString(payload)
  }

  if (!isJsonRecord(payload)) {
    return null
  }

  const directMessage = firstNonEmptyString([
    payload.message,
    payload.error,
    payload.details,
  ])

  if (directMessage) {
    return directMessage
  }

  return null
}
