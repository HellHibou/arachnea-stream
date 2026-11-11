import { isDesktopApp } from '@/services/rustify/desktop'
import type {
  PreparedSubtitleTranslation,
  SubtitleTranslationFile,
} from '@/types/translation'

/** Maximum number of translated files retained by the application cache. */
const SUBTITLE_FILE_CACHE_MAX_ENTRIES = 12
/** Maximum total bytes retained by the application cache. */
const SUBTITLE_FILE_CACHE_MAX_BYTES = 8 * 1024 * 1024
/** Bytes inspected to reject non-WebVTT error bodies before playback. */
const WEBVTT_SIGNATURE_BYTES = 64

/** In-memory ETag-to-body cache used when the WebView HTTP cache is unavailable. */
const subtitleFileCache = new Map<string, CachedSubtitleFile>()
/** Monotonic access counter implementing LRU eviction. */
let accessSequence = 0
/** Current approximate cache size in bytes. */
let cachedBytes = 0

/** One complete translated file retained with its composite validator. */
interface CachedSubtitleFile {
  /** Complete WebVTT document. */
  blob: Blob
  /** Validator returned by the file transport. */
  etag: string
  /** Access order used for LRU eviction. */
  access: number
}

/** Result of one successful file download. */
export interface SubtitleTranslationDownload extends SubtitleTranslationFile {
  /** Whether the body came from the application cache instead of a new response. */
  fromCache: boolean
}

/** Failure category exposed to the translation composable. */
export type SubtitleTranslationFileFailureKind = 'http' | 'invalid-response' | 'network'

/** Error raised while downloading one translated subtitle file. */
export class SubtitleTranslationFileError extends Error {
  /** Failure category. */
  readonly kind: SubtitleTranslationFileFailureKind
  /** HTTP status when the failure came from a response. */
  readonly status: number | null

  /**
   * Creates one file transport error.
   *
   * @param message Developer-facing diagnostic message.
   * @param kind Failure category.
   * @param status Optional HTTP status.
   */
  constructor(message: string, kind: SubtitleTranslationFileFailureKind, status: number | null = null) {
    super(message)
    this.name = 'SubtitleTranslationFileError'
    this.kind = kind
    this.status = status
  }
}

/**
 * Downloads one prepared translated subtitle and validates it before playback.
 *
 * Conditional requests reuse the previous body only when the server proves the
 * representation is unchanged. A `304` received without a retained body is
 * retried without a validator instead of exposing an unusable empty file.
 *
 * @param prepared Prepared track returned by `prepare_subtitle_translation`.
 * @param options Abort signal owned by the caller.
 * @returns Complete WebVTT file and its composite validator.
 */
export async function fetchSubtitleTranslationFile(
  prepared: PreparedSubtitleTranslation,
  options: { signal?: AbortSignal } = {},
): Promise<SubtitleTranslationDownload> {
  const url = resolveSubtitleTranslationUrl(prepared.link)
  const cached = readCachedSubtitleFile(url)
  let response = await requestSubtitleFile(url, cached?.etag ?? null, options.signal, false)

  if (response.status === 304) {
    if (cached) {
      const etag = response.headers.get('ETag') ?? cached.etag
      writeCachedSubtitleFile(url, etag, cached.blob)
      return {
        ...toDownload(cached.blob, etag, prepared),
        fromCache: true,
      }
    }

    response = await requestSubtitleFile(url, null, options.signal, true)
  }

  if (!response.ok) {
    if (response.status === 403 || response.status === 404 || response.status === 410) {
      deleteCachedSubtitleFile(url)
    }
    throw new SubtitleTranslationFileError(
      `Subtitle translation request failed with status ${response.status}.`,
      'http',
      response.status,
    )
  }

  const blob = await readWebVttResponse(response)
  const etag = response.headers.get('ETag')

  if (etag) {
    writeCachedSubtitleFile(url, etag, blob)
  } else {
    deleteCachedSubtitleFile(url)
  }

  return {
    ...toDownload(blob, etag, prepared),
    fromCache: false,
  }
}

/**
 * Clears every translated file retained in memory.
 *
 * The browser's native HTTP cache is managed by the browser and is not cleared
 * by this function; its entries revalidate against the server before reuse.
 */
export function clearSubtitleTranslationFileCache(): void {
  subtitleFileCache.clear()
  cachedBytes = 0
}

/**
 * Resolves a prepared link into a fetchable URL and rejects unsafe schemes.
 *
 * @param link Backend-provided tokenized file URL.
 * @returns Absolute URL accepted by the current transport.
 */
export function resolveSubtitleTranslationUrl(link: string): string {
  let url: URL
  try {
    url = new URL(link, window.location.href)
  } catch {
    throw new SubtitleTranslationFileError(
      'The translated subtitle link is not a valid URL.',
      'invalid-response',
    )
  }

  if (url.protocol === 'http:' || url.protocol === 'https:') {
    return url.href
  }

  if (isDesktopApp() && !['blob:', 'data:', 'file:', 'javascript:'].includes(url.protocol)) {
    return url.href
  }

  throw new SubtitleTranslationFileError(
    `Unsupported translated subtitle URL scheme ${url.protocol}.`,
    'invalid-response',
  )
}

/**
 * Sends one file request with optional conditional validation.
 *
 * @param url Absolute tokenized file URL.
 * @param etag Previous composite validator, or `null` for an unconditional request.
 * @param signal Caller-owned abort signal.
 * @param bypassCache Whether the request must avoid the native HTTP cache.
 * @returns Raw fetch response.
 */
async function requestSubtitleFile(
  url: string,
  etag: string | null,
  signal: AbortSignal | undefined,
  bypassCache: boolean,
): Promise<Response> {
  const headers = new Headers({ Accept: 'text/vtt' })
  if (etag) {
    headers.set('If-None-Match', etag)
  }

  try {
    return await fetch(url, {
      method: 'GET',
      headers,
      signal,
      credentials: 'same-origin',
      cache: bypassCache ? 'reload' : 'no-cache',
    })
  } catch (error) {
    if (isAbortError(error)) {
      throw error
    }

    throw new SubtitleTranslationFileError(
      error instanceof Error ? error.message : 'Subtitle translation download failed.',
      'network',
    )
  }
}

/**
 * Reads and validates a WebVTT response body.
 *
 * @param response Successful file response.
 * @returns WebVTT-typed blob containing the complete document.
 */
async function readWebVttResponse(response: Response): Promise<Blob> {
  const contentType = response.headers.get('Content-Type')?.toLocaleLowerCase() ?? ''
  if (!contentType.startsWith('text/vtt')) {
    throw new SubtitleTranslationFileError(
      `Unexpected translated subtitle content type ${contentType || 'missing'}.`,
      'invalid-response',
      response.status,
    )
  }

  const blob = await response.blob()
  if (blob.size === 0) {
    throw new SubtitleTranslationFileError(
      'The translated subtitle file is empty.',
      'invalid-response',
      response.status,
    )
  }

  const signature = (await blob.slice(0, WEBVTT_SIGNATURE_BYTES).text())
    .replace(/^\uFEFF/, '')
    .trimStart()
  if (!signature.startsWith('WEBVTT')) {
    throw new SubtitleTranslationFileError(
      'The translated subtitle file is not a WebVTT document.',
      'invalid-response',
      response.status,
    )
  }

  return blob.type.toLocaleLowerCase().startsWith('text/vtt')
    ? blob
    : new Blob([blob], { type: 'text/vtt; charset=utf-8' })
}

/**
 * Builds the file payload consumed by the track owner.
 *
 * @param blob Complete WebVTT document.
 * @param etag Composite validator, if returned.
 * @param prepared Prepared track metadata.
 * @returns File metadata ready for the translation composable.
 */
function toDownload(
  blob: Blob,
  etag: string | null,
  prepared: PreparedSubtitleTranslation,
): SubtitleTranslationFile {
  return {
    blob,
    lang: prepared.lang,
    label: prepared.label,
    etag,
  }
}

/**
 * Reads one cached file and refreshes its LRU position.
 *
 * @param url Absolute file URL.
 * @returns Cached entry, or `undefined` when no body is retained.
 */
function readCachedSubtitleFile(url: string): CachedSubtitleFile | undefined {
  const entry = subtitleFileCache.get(url)
  if (!entry) {
    return undefined
  }

  entry.access = ++accessSequence
  subtitleFileCache.delete(url)
  subtitleFileCache.set(url, entry)
  return entry
}

/**
 * Stores one file body and evicts least-recently-used entries beyond the bounds.
 *
 * @param url Absolute file URL.
 * @param etag Composite validator.
 * @param blob Complete WebVTT document.
 */
function writeCachedSubtitleFile(url: string, etag: string, blob: Blob): void {
  if (blob.size > SUBTITLE_FILE_CACHE_MAX_BYTES) {
    deleteCachedSubtitleFile(url)
    return
  }

  deleteCachedSubtitleFile(url)
  subtitleFileCache.set(url, { blob, etag, access: ++accessSequence })
  cachedBytes += blob.size

  while (
    subtitleFileCache.size > SUBTITLE_FILE_CACHE_MAX_ENTRIES ||
    cachedBytes > SUBTITLE_FILE_CACHE_MAX_BYTES
  ) {
    const oldestKey = subtitleFileCache.keys().next().value
    if (oldestKey === undefined) {
      break
    }
    deleteCachedSubtitleFile(oldestKey)
  }
}

/**
 * Removes one cached file body.
 *
 * @param url Absolute file URL.
 */
function deleteCachedSubtitleFile(url: string): void {
  const entry = subtitleFileCache.get(url)
  if (!entry) {
    return
  }

  subtitleFileCache.delete(url)
  cachedBytes -= entry.blob.size
  if (cachedBytes < 0) {
    cachedBytes = 0
  }
}

/**
 * Checks whether an error is a cancelled fetch.
 *
 * @param error Unknown thrown value.
 * @returns True for AbortError instances.
 */
function isAbortError(error: unknown): boolean {
  return error instanceof Error && error.name === 'AbortError'
}
