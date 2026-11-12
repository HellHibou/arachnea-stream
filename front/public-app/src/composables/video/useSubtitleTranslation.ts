import { computed, onScopeDispose, shallowRef } from 'vue'

import { t } from '@/i18n'
import { getBaseLanguageCode, normalizeLanguageCode } from '@/services/languages'
import {
  fetchSubtitleTranslationFile,
  SubtitleTranslationFileError,
} from '@/services/subtitleTranslationClient'
import type {
  SubtitleTranslationFile,
  SubtitleTranslationLanguageRequest,
} from '@/types/translation'

/** Lifecycle of one requested translated subtitle. */
export type SubtitleTranslationStatus =
  | 'idle'
  | 'preparing'
  | 'downloading'
  | 'ready'
  | 'error'

/** Failure exposed by the translation composable. */
export interface SubtitleTranslationFailure {
  /** Stage that failed. */
  stage: 'prepare' | 'download' | 'apply'
  /** HTTP status when the failure came from a file response. */
  status: number | null
  /** Localized message ready for the user interface. */
  message: string
  /** Original thrown value kept for diagnostics. */
  cause: unknown
}

/** Track owner receiving a successfully downloaded WebVTT file. */
export interface SubtitleTranslationTrackHost {
  /**
   * Applies one translated file and selects its track.
   *
   * @param file Complete WebVTT file and track metadata.
   * @param options Application options.
   * @returns True when the player accepted and selected the track.
   */
  apply(
    file: SubtitleTranslationFile,
    options?: { origin?: 'manual' | 'profile' },
  ): boolean
  /** Removes the translated track and restores the previous subtitle choice. */
  clear(): void
  /** Removes the translated track without restoring the replaced subtitle choice. */
  discard(): void
}

/** Options accepted by one subtitle translation request. */
export interface SubtitleTranslationRequestOptions {
  /** Origin recorded when player state is captured. */
  origin?: 'manual' | 'profile'
}

/** Options accepted by the subtitle translation composable. */
export interface UseSubtitleTranslationOptions {
  /** Video.js-backed track owner. */
  host: SubtitleTranslationTrackHost
  /** Optional observer invoked once per user-visible failure. */
  onFailure?: (failure: SubtitleTranslationFailure) => void
}

/**
 * Prepares and downloads one translated subtitle with cancellation and stale-response protection.
 *
 * The composable owns no player instance, token or Blob URL: a new request
 * invalidates the previous one, and the track host owns the blob lifecycle.
 *
 * @param options Track host and optional failure observer.
 * @returns State and explicit lifecycle operations for one player instance.
 */
export function useSubtitleTranslation(options: UseSubtitleTranslationOptions) {
  /** Current request state. */
  const status = shallowRef<SubtitleTranslationStatus>('idle')
  /** Last user-visible failure, or `null` after a successful request or reset. */
  const failure = shallowRef<SubtitleTranslationFailure | null>(null)
  /** Language of the current request or applied translation. */
  const targetLanguage = shallowRef<string | null>(null)
  /** Whether a preparation or download is currently active. */
  const isLoading = computed(() =>
    status.value === 'preparing' || status.value === 'downloading',
  )

  /** Monotonic request identifier used to ignore stale responses. */
  let requestId = 0
  /** Controller for the active preparation/download operation. */
  let abortController: AbortController | null = null

  /**
   * Requests one translated subtitle and applies it after a complete download.
   *
   * @param request Language request produced by the subtitle preference layer.
   * @param requestOptions Request origin used by state capture.
   * @returns Applied file, or `null` when cancelled, superseded or failed.
   */
  async function request(
    request: SubtitleTranslationLanguageRequest,
    requestOptions: SubtitleTranslationRequestOptions = {},
  ): Promise<SubtitleTranslationFile | null> {
    const operationId = ++requestId
    abortController?.abort()
    const controller = new AbortController()
    abortController = controller
    options.host.discard()
    status.value = 'preparing'
    failure.value = null
    targetLanguage.value = null

    const target = toTranslationLanguageCode(request.targetLanguage)
    if (!request.source.url.trim() || !target) {
      return fail(
        operationId,
        'prepare',
        null,
        new Error('Invalid subtitle translation request.'),
        controller,
      )
    }
    targetLanguage.value = target

    const sourceLanguage = toTranslationLanguageCode(request.sourceLanguage)

    try {
      status.value = 'downloading'
      const downloaded = await fetchSubtitleTranslationFile(
        { ...request, targetLanguage: target, sourceLanguage },
        { signal: controller.signal },
      )
      if (!isCurrent(operationId)) {
        return null
      }

      const label = request.label?.trim() || downloaded.label
      const file: SubtitleTranslationFile = {
        blob: downloaded.blob,
        lang: downloaded.lang,
        label,
        etag: downloaded.etag,
      }

      if (!options.host.apply(file, { origin: requestOptions.origin ?? 'profile' })) {
        return fail(
          operationId,
          'apply',
          null,
          new Error('The player is not ready to receive the translated subtitle.'),
          controller,
        )
      }

      targetLanguage.value = downloaded.lang
      status.value = 'ready'
      return file
    } catch (error) {
      if (!isCurrent(operationId) || controller.signal.aborted || isAbortError(error)) {
        return null
      }

      return fail(
        operationId,
        status.value === 'downloading' ? 'download' : 'prepare',
        error instanceof SubtitleTranslationFileError ? error.status : null,
        error,
        controller,
      )
    } finally {
      if (isCurrent(operationId) && abortController === controller) {
        abortController = null
      }
    }
  }

  /** Cancels the active operation and removes the translated track without restoring. */
  function cancel(): void {
    requestId += 1
    abortController?.abort()
    abortController = null
    options.host.discard()
    status.value = 'idle'
    failure.value = null
    targetLanguage.value = null
  }

  /** Alias used by renderer teardown and source changes. */
  function reset(): void {
    cancel()
  }

  /** Dismisses a translation failure without changing the current subtitle track. */
  function clearFailure(): void {
    failure.value = null
    if (status.value === 'error') {
      status.value = 'idle'
      targetLanguage.value = null
    }
  }

  /**
   * Checks whether one operation is still the active request.
   *
   * @param operationId Operation identifier.
   * @returns True while no newer request or cancellation superseded it.
   */
  function isCurrent(operationId: number): boolean {
    return operationId === requestId
  }

  /**
   * Records one failure unless the operation was superseded.
   *
   * @param operationId Operation identifier.
   * @param stage Failure stage.
   * @param httpStatus Optional HTTP status.
   * @param error Original error.
   * @param controller Operation controller, when still active.
   * @returns Always `null`.
   */
  function fail(
    operationId: number,
    stage: SubtitleTranslationFailure['stage'],
    httpStatus: number | null,
    error: unknown,
    controller: AbortController | null,
  ): null {
    if (!isCurrent(operationId)) {
      return null
    }

    if (controller && abortController === controller) {
      abortController = null
    }
    const nextFailure: SubtitleTranslationFailure = {
      stage,
      status: httpStatus,
      message: describeFailure(error, httpStatus),
      cause: error,
    }
    failure.value = nextFailure
    status.value = 'error'
    options.onFailure?.(nextFailure)
    return null
  }

  onScopeDispose(cancel)

  return {
    status,
    failure,
    targetLanguage,
    isLoading,
    request,
    cancel,
    reset,
    clearFailure,
  }
}

/**
 * Normalizes one language code for the two-letter translation contract.
 *
 * Three-letter or malformed codes are rejected instead of being truncated.
 *
 * @param value Raw language code.
 * @returns Lowercase two-letter code, or `null` when unsupported.
 */
function toTranslationLanguageCode(value: unknown): string | null {
  const base = getBaseLanguageCode(normalizeLanguageCode(value))
  return base && /^[a-z]{2}$/.test(base) ? base : null
}

/**
 * Builds a localized user-facing message for one translation failure.
 *
 * @param error Original thrown value.
 * @param httpStatus Optional file-response status.
 * @returns Localized failure message.
 */
function describeFailure(error: unknown, httpStatus: number | null): string {
  if (error instanceof SubtitleTranslationFileError) {
    switch (httpStatus) {
      case 403:
        return t('errors.subtitleTranslationAccessDenied')
      case 404:
      case 410:
        return t('errors.subtitleTranslationExpired')
      case 502:
      case 503:
      case 504:
        return t('errors.subtitleTranslationSourceUnavailable')
      default:
        break
    }

    if (error.kind === 'invalid-response') {
      return t('errors.subtitleTranslationInvalidResponse')
    }
    if (error.kind === 'network') {
      return t('errors.subtitleTranslationNetwork')
    }
  }

  const message = error instanceof Error ? error.message.trim() : ''
  return message || t('errors.subtitleTranslationFailed')
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
