import { t } from '@/i18n'
import { captureSessionFailureReporter, sessionGeneration } from '@/services/sessionEvents'
import { resolveAppPath } from '@/services/baseUrl'
import { enqueueErrorNotification, reportFrontendErrorNotification } from '@/composables/useErrorNotifications'
import type {
  ScraperAggregationResult,
  ScraperExecutionError,
} from '@/types/scraperError'

import { firstNonEmptyString, isJsonRecord, normalizeString } from './primitives/readers'
import type { TauriWindow } from './desktop'

/** Base URL for the REST API. Falls back to the `api` segment under the app base when not configured via environment. */
const restApiBaseUrl = import.meta.env.VITE_RUSTIFY_API_BASE_URL ?? resolveAppPath('api')

/** Reserved invoke payload field used by native IPC to carry the client ETag. */
const nativeEtagField = 'arachneaEtag'

/** Top-level key of the conditional reply wrapper returned by native IPC. */
const nativeConditionalReplyField = '__arachnea_conditional_reply'

/** Cached conditional-validation entry keyed by command name and serialized parameters. */
interface EtagCacheEntry {
  /** Global ETag returned by the backend for this request. */
  etag: string
  /** Raw JSON envelope previously returned for this request. */
  payload: unknown
}

/** In-memory conditional-validation cache shared by REST GET and native invoke calls. */
const etagCache = new Map<string, EtagCacheEntry>()
window.addEventListener('arachnea-session-context', () => etagCache.clear())
window.addEventListener('arachnea-profile-context', () => etagCache.clear())

/**
 * Builds the cache key of one backend call from its command and parameters.
 *
 * @param fct_name Backend function name.
 * @param serializedParams Serialized parameters of the request.
 * @returns Stable cache key scoped to this exact request.
 */
function buildEtagCacheKey(fct_name: string, serializedParams: string): string {
  return `${fct_name}?${serializedParams}`
}

/** Conditional reply relayed by native IPC when a command produced a validator. */
interface NativeConditionalReply {
  /** Whether the backend answered `304 Not Modified`. */
  notModified: boolean
  /** Validator returned by the backend, or null when none was produced. */
  etag: string | null
  /** Command payload to cache when the reply was not a `304`. */
  value: unknown
}

/**
 * Recognizes the conditional reply wrapper emitted by the native IPC transport.
 *
 * @param payload Deserialized invoke reply.
 * @returns The conditional reply, or null for a bare command payload.
 */
function readNativeConditionalReply(payload: unknown): NativeConditionalReply | null {
  if (!isJsonRecord(payload)) return null
  const reply = payload[nativeConditionalReplyField]
  if (!isJsonRecord(reply) || typeof reply.not_modified !== 'boolean' || !('value' in reply)) return null
  return {
    notModified: reply.not_modified,
    etag: typeof reply.etag === 'string' ? reply.etag : null,
    value: reply.value,
  }
}

/**
 * Stores or clears the cached validator and payload for one request shape.
 *
 * @param cacheKey Stable key of the request shape.
 * @param etag Raw ETag returned by the backend, or null when none was produced.
 * @param payload Payload associated with the validator.
 */
function storeEtagEntry(cacheKey: string, etag: string | null, payload: unknown): void {
  if (etag) {
    etagCache.set(cacheKey, {
      etag: etag.trim().replace(/^W\//, '').replace(/^"|"$/g, ''),
      payload,
    })
  } else {
    etagCache.delete(cacheKey)
  }
}

/**
 * Calls a backend function through either Tauri or the REST bridge.
 *
 * The REST path issues a plain GET request whose parameters are JSON-encoded
 * into the query string: the browser's private HTTP cache stores ETag-bearing
 * responses and replays `If-None-Match` on the next load, resolving a `304`
 * with the cached body before this function sees the reply.
 * The Tauri path has no HTTP cache, so it carries the validator in the reserved
 * `arachneaEtag` invoke field, receives a conditional reply with the new ETag
 * and a not-modified flag, and replays its cached payload on `304`.
 * The native ETag cache is cleared whenever the session or selected profile changes.
 *
 * @param fct_name Backend function name.
 * @param params Function parameters.
 * @param options Set rejectExecutionErrors for requests that must succeed before advancing a cursor.
 * @returns Deserialized backend payload.
 */
export async function call_api<T = unknown>(
  fct_name: string,
  params: Record<string, unknown>,
  options: { rejectExecutionErrors?: boolean } = {},
): Promise<T> {
  const generation = sessionGeneration()
  const reportSessionFailure = captureSessionFailureReporter()
  const tauriWindow = window as TauriWindow
  const tauriInvoke =
    tauriWindow.__TAURI__?.core?.invoke ?? tauriWindow.__TAURI__?.invoke

  try {
    if (tauriInvoke) {
      const cacheKey = buildEtagCacheKey(fct_name, JSON.stringify(params))
      const cachedEntry = etagCache.get(cacheKey)
      const invokeParams = cachedEntry
        ? { ...params, [nativeEtagField]: cachedEntry.etag }
        : params
      const reply = await tauriInvoke<unknown>(fct_name, invokeParams)
      if (generation !== sessionGeneration()) throw new ReportedApiError('Discarded response from a previous account.')

      const conditional = readNativeConditionalReply(reply)
      if (conditional) {
        const notModified = conditional.notModified && cachedEntry !== undefined
        const payload = notModified ? cachedEntry.payload : conditional.value
        checkSessionReply(payload, generation, reportSessionFailure)
        if (!notModified) storeEtagEntry(cacheKey, conditional.etag, conditional.value)
        return unwrapAggregationPayload<T>(payload, fct_name, options.rejectExecutionErrors)
      }

      checkSessionReply(reply, generation, reportSessionFailure)
      return unwrapAggregationPayload<T>(reply, fct_name, options.rejectExecutionErrors)
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
    const response = await fetch(
      `${restApiBaseUrl}/${fct_name}${queryString ? `?${queryString}` : ''}`,
      { method: 'GET', credentials: 'same-origin' },
    )

    if (generation !== sessionGeneration()) throw new ReportedApiError('Discarded response from a previous account.')
    const payload = await parseResponseBody(response)
    checkSessionReply(payload, generation, reportSessionFailure)

    if (!response.ok) {
      handleFailedRestResponse(payload, fct_name, response.status)
    }

    return unwrapAggregationPayload<T>(payload, fct_name, options.rejectExecutionErrors)
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
function unwrapAggregationPayload<T>(payload: unknown, operation: string, rejectExecutionErrors = false): T {
  const aggregationPayload = readAggregationPayload<T>(payload)

  if (aggregationPayload.kind === 'malformed') {
    return throwFrontendApiError(operation, 'Malformed scraper aggregation response envelope.')
  }

  if (aggregationPayload.kind === 'bare') {
    return payload as T
  }

  reportExecutionErrors(aggregationPayload.value.errors)
  if (rejectExecutionErrors && aggregationPayload.value.errors.length) {
    throw new ReportedApiError(aggregationPayload.value.errors.map((error) => error.message).join('\n'))
  }
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
  reportFrontendErrorNotification(operation, message)
  throw new ReportedApiError(message)
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

/** Rejects late replies and reports authentication failures. @param payload Server response. @param generation Original account generation. @param reportSessionFailure Reporter for the credential used by the request. */
function checkSessionReply(payload: unknown, generation: number, reportSessionFailure: (code: string) => void): void {
  if (generation !== sessionGeneration()) throw new ReportedApiError('Discarded response from a previous account.')
  if (isJsonRecord(payload) && isJsonRecord(payload.error) && typeof payload.error.code === 'string') {
    reportSessionFailure(payload.error.code)
    throw new ReportedApiError(typeof payload.error.message === 'string' ? payload.error.message : 'Session is unavailable.')
  }
}
