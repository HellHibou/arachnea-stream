import { shallowRef, watch } from 'vue'

import { t } from '@/i18n'
import {
  getBaseLanguageCode,
  getLanguageNativeName,
  normalizeLanguageCode,
} from '@/services/languages'
import type { ResolvedVideoMediaSource } from '@/services/players'
import { getTranslationServices } from '@/services/rustify'
import type { TranslationCatalog, TranslationServiceCapabilities } from '@/types/translation'
import type { VideoJsPlayerState } from '@/types/media'

import { getAppliedTextTrackOrigin } from './video-js-media-renderer/state'
import type { VideoJsPlayer } from './video-js-media-renderer/types'
import {
  planPrioritySubtitleSelection,
  readDisplayTextTracks,
  resolveTrackProfile,
  type VideoJsTrackSelectionConfig,
} from './video-js-media-renderer/trackPreferences'
import {
  useSubtitleTranslation,
  type SubtitleTranslationTrackHost,
} from './useSubtitleTranslation'

/** A translation target offered by the native subtitle menu. */
export interface SubtitleTranslationMenuTarget {
  /** Two-letter target language. */
  language: string
  /** Localized action label. */
  label: string
}

/** Options accepted by the subtitle translation controller. */
export interface UseSubtitleTranslationControllerOptions {
  /** Returns the active Video.js player. */
  getPlayer: () => VideoJsPlayer | null
  /** Returns the source currently bound to the renderer. */
  getSource: () => ResolvedVideoMediaSource | null | undefined
  /** Returns the active track selection configuration. */
  getTrackSelectionConfig: () => VideoJsTrackSelectionConfig | null | undefined
  /** Returns the ordered favorite subtitle languages. */
  getSubtitleLanguagePriority: () => readonly string[]
  /** Returns the per-language translation authorization map. */
  getTranslationByLanguage: () => Readonly<Record<string, boolean>>
  /** Track owner receiving downloaded translations. */
  host: SubtitleTranslationTrackHost
  /** Invoked when menu entries may have changed. */
  onMenuInvalidated?: () => void
}

/** Shared catalog promise so services are loaded once per application session. */
let catalogPromise: Promise<TranslationCatalog | null> | null = null

/**
 * Orchestrates automatic and manual subtitle translation for one renderer.
 *
 * @param options Player, source, preferences and track host accessors.
 * @returns Translation lifecycle state plus menu and player integration methods.
 */
export function useSubtitleTranslationController(
  options: UseSubtitleTranslationControllerOptions,
) {
  const translation = useSubtitleTranslation({ host: options.host })
  const catalog = shallowRef<TranslationCatalog | null>(null)

  /** Whether a manual subtitle choice must keep automatic generation disabled. */
  let manualMode = false
  /** Per-source keys already attempted automatically. */
  const failedRequestKeys = new Set<string>()
  /** Prevents overlapping automatic evaluations for the same player. */
  let evaluating = false
  /** Invalidates evaluation continuations after a reset. */
  let evaluationEpoch = 0
  /** Invalidates pending manual requests after a user choice or reset. */
  let manualRequestId = 0

  /**
   * Loads the public translation catalog once per application session.
   *
   * A failed load is cached as unavailable so automatic and manual generation
   * stay disabled until the application is reloaded.
   *
   * @returns Catalog, or `null` when the backend does not expose one.
   */
  async function ensureCatalog(): Promise<TranslationCatalog | null> {
    if (catalog.value) {
      return catalog.value
    }

    if (!catalogPromise) {
      catalogPromise = getTranslationServices().catch(() => null)
    }

    const loaded = await catalogPromise
    if (loaded && catalog.value !== loaded) {
      catalog.value = loaded
      options.onMenuInvalidated?.()
    }

    return loaded
  }

  /**
   * Returns the authorized two-letter target languages in favorite order.
   *
   * @returns Unique authorized target codes.
   */
  function readAuthorizedLanguages(): string[] {
    const authorization = options.getTranslationByLanguage()
    const result: string[] = []
    const seen = new Set<string>()

    for (const priority of options.getSubtitleLanguagePriority()) {
      const target = toTranslationCode(priority)
      if (!target || seen.has(target) || authorization[target] !== true) continue
      seen.add(target)
      result.push(target)
    }

    return result
  }

  /** Invalidates automatic evaluation continuations and releases their guard. */
  function invalidateEvaluations(): void {
    evaluationEpoch += 1
    manualRequestId += 1
    evaluating = false
  }

  /**
   * Finds the external subtitle entry used as translation source.
   *
   * The currently shown human track wins, then the first human track, then the
   * first external entry. Renderer-owned translated Blob tracks are never used.
   *
   * @param player Active player.
   * @param source Active media source.
   * @returns Source entry and optional detected language.
   */
  function resolveSource(player: VideoJsPlayer, source: ResolvedVideoMediaSource) {
    const entries = source.subtitles.filter((subtitle) => subtitle.link.trim())
    if (!entries.length) {
      return null
    }

    const humanEntries = entries.filter((subtitle) => subtitle.autoTranslate !== true)
    const current = readDisplayTextTracks(player).find(
      (track) => track.mode === 'showing' && track.autoTranslate !== true,
    )
    const currentLanguage = getBaseLanguageCode(normalizeLanguageCode(current?.language))
    const currentLabel = current?.label?.trim()
    const currentEntry = current
      ? humanEntries.find(
        (subtitle) => currentLabel
          && subtitle.label?.trim() === currentLabel
          && getBaseLanguageCode(normalizeLanguageCode(subtitle.lang)) === currentLanguage,
      ) ?? humanEntries.find(
        (subtitle) => currentLanguage
          && getBaseLanguageCode(normalizeLanguageCode(subtitle.lang)) === currentLanguage,
      )
      : null
    const entry = currentEntry ?? humanEntries[0] ?? entries[0]
    if (!entry) {
      return null
    }

    const url = toAbsoluteHttpUrl(entry.link)
    if (!url) {
      return null
    }

    return {
      source: {
        url,
        format: 'webvtt' as const,
      },
      sourceLanguage: toTranslationCode(entry.lang),
    }
  }

  /**
   * Checks whether one service can translate a source/target pair.
   *
   * @param service Service capabilities.
   * @param sourceLanguage Known two-letter source language, or `null`.
   * @param targetLanguage Two-letter target language.
   * @returns True when the backend can route this pair through the service.
   */
  function serviceSupportsPair(
    service: TranslationServiceCapabilities,
    sourceLanguage: string | null,
    targetLanguage: string,
  ): boolean {
    if (!service.supportsTextTranslation && !service.supportsUrlTranslation) {
      return false
    }
    if (!languageMatches(service.targetLanguages, targetLanguage)) {
      return false
    }

    if (sourceLanguage) {
      if (!languageMatches(service.sourceLanguages, sourceLanguage)) {
        return false
      }
    } else if (!service.sourceDetection) {
      return false
    }

    if (!service.supportedPairs) {
      return true
    }

    return service.supportedPairs.some((pair) => {
      const targetMatches = toTranslationCode(pair.target) === targetLanguage
      if (!targetMatches) return false
      return sourceLanguage ? toTranslationCode(pair.source) === sourceLanguage : true
    })
  }

  /**
   * Checks whether any catalog service supports one pair.
   *
   * @param sourceLanguage Known source language, or `null`.
   * @param targetLanguage Two-letter target language.
   * @returns True when at least one service can translate the pair.
   */
  function hasPairSupport(sourceLanguage: string | null, targetLanguage: string): boolean {
    const services = catalog.value?.services ?? []
    return services.some((service) => serviceSupportsPair(service, sourceLanguage, targetLanguage))
  }

  /**
   * Builds the translation label used by the generated subtitle track.
   *
   * @param language Two-letter target language.
   * @returns Localized track label.
   */
  function buildTrackLabel(language: string): string {
    return t('player.subtitleTranslation.trackLabel', {
      language: getLanguageNativeName(language) ?? language.toLocaleUpperCase(),
    })
  }

  /**
   * Requests automatic translation for the first authorized missing favorite.
   *
   * @param player Player whose restored profile should be translated.
   */
  async function evaluateAuto(player: VideoJsPlayer): Promise<void> {
    if (evaluating || manualMode) {
      return
    }
    if (translation.status.value === 'error') {
      return
    }
    if (getAppliedTextTrackOrigin(player) === 'manual') {
      manualMode = true
      return
    }

    const config = options.getTrackSelectionConfig()
    const source = options.getSource()
    if (!config || !source) {
      return
    }

    const resolved = resolveTrackProfile(player, config.profiles, config.allowAudioDescription)
    if (resolved.profile.subtitles !== 'auto') {
      return
    }

    const authorized = new Set(readAuthorizedLanguages())
    if (!authorized.size) {
      return
    }

    const translationSource = resolveSource(player, source)
    if (!translationSource) {
      return
    }

    const sourceLanguage = translationSource.sourceLanguage
    const epoch = evaluationEpoch
    evaluating = true
    try {
      if (!await ensureCatalog()) {
        return
      }
      if (
        epoch !== evaluationEpoch
        || manualMode
        || translation.isLoading.value
        || options.getPlayer() !== player
        || options.getSource() !== source
      ) {
        return
      }

      const plan = planPrioritySubtitleSelection(
        player,
        config.subtitleLanguagePriority,
        (language) => authorized.has(language) && hasPairSupport(sourceLanguage, language),
      )
      const targetLanguage = plan.translationLanguage
      if (translation.isLoading.value) {
        if (targetLanguage === translation.targetLanguage.value) {
          return
        }
        translation.cancel()
      } else if (translation.status.value === 'ready') {
        if (plan.track && plan.track.autoTranslate !== true) {
          translation.cancel()
        }
        return
      }

      if (!targetLanguage) {
        return
      }

      const requestKey = buildRequestKey(
        translationSource.source.url,
        sourceLanguage,
        targetLanguage,
      )
      if (failedRequestKeys.has(requestKey)) {
        return
      }

      await translation.request(
        {
          source: translationSource.source,
          targetLanguage,
          sourceLanguage,
          policy: { mode: 'auto' },
          label: buildTrackLabel(targetLanguage),
        },
        { origin: 'profile' },
      )

      if (translation.failure.value) {
        failedRequestKeys.add(requestKey)
      }
    } finally {
      if (epoch === evaluationEpoch) {
        evaluating = false
      }
    }
  }

  /**
   * Returns languages offered by the native subtitle menu.
   *
   * Manual generation is independent from the per-language automatic
   * authorization; the explicit click itself authorizes the translation.
   *
   * @returns Unique favorite targets with no existing track and catalog support.
   */
  function getManualTargets(): SubtitleTranslationMenuTarget[] {
    const player = options.getPlayer()
    const source = options.getSource()
    if (!player || !source) {
      return []
    }

    void ensureCatalog()
    const translationSource = resolveSource(player, source)
    if (!translationSource) {
      return []
    }

    const existingLanguages = new Set<string>()
    for (const track of readDisplayTextTracks(player)) {
      const language = toTranslationCode(track.language)
      if (language) existingLanguages.add(language)
    }

    const activeTarget = translation.isLoading.value ? translation.targetLanguage.value : null
    const targets: SubtitleTranslationMenuTarget[] = []
    const seen = new Set<string>()

    for (const priority of options.getSubtitleLanguagePriority()) {
      const language = toTranslationCode(priority)
      if (
        !language
        || seen.has(language)
        || existingLanguages.has(language)
        || language === activeTarget
      ) {
        continue
      }
      if (!hasPairSupport(translationSource.sourceLanguage, language)) {
        continue
      }

      seen.add(language)
      targets.push({
        language,
        label: t('player.subtitleTranslation.generate', {
          language: getLanguageNativeName(language) ?? language.toLocaleUpperCase(),
        }),
      })
    }

    return targets
  }

  /**
   * Starts an explicit manual translation from the subtitle menu.
   *
   * Manual requests cancel any pending automatic request and persist their
   * origin as a manual subtitle choice.
   *
   * @param language Two-letter target language.
   */
  async function requestManual(language: string): Promise<void> {
    const player = options.getPlayer()
    const source = options.getSource()
    const target = toTranslationCode(language)
    if (!player || !source || !target) {
      return
    }

    const translationSource = resolveSource(player, source)
    if (!translationSource) {
      return
    }

    const requestId = ++manualRequestId
    manualMode = true
    translation.clearFailure()
    await ensureCatalog()
    if (
      requestId !== manualRequestId
      || options.getPlayer() !== player
      || options.getSource() !== source
    ) {
      return
    }
    if (!hasPairSupport(translationSource.sourceLanguage, target)) {
      return
    }

    await translation.request(
      {
        source: translationSource.source,
        targetLanguage: target,
        sourceLanguage: translationSource.sourceLanguage,
        policy: { mode: 'auto' },
        label: buildTrackLabel(target),
      },
      { origin: 'manual' },
    )
  }

  /**
   * Cancels pending work after the active source changes.
   */
  function resetForSource(): void {
    manualMode = false
    invalidateEvaluations()
    failedRequestKeys.clear()
    translation.reset()
  }

  /**
   * Reapplies automatic generation after a settings change.
   */
  function resetForPreferences(): void {
    manualMode = false
    invalidateEvaluations()
    failedRequestKeys.clear()
    translation.reset()
  }

  /**
   * Releases pending work when the player is destroyed.
   */
  function resetForPlayer(): void {
    manualMode = false
    invalidateEvaluations()
    failedRequestKeys.clear()
    translation.reset()
  }

  /**
   * Detects manual subtitle choices and cancels/removes translated tracks.
   *
   * @param state Latest captured player state.
   */
  function handlePlayerState(state: VideoJsPlayerState): void {
    if (state.textTrack.origin !== 'manual') {
      return
    }

    manualMode = true
    manualRequestId += 1
    if (state.textTrack.autoTranslate === true) {
      if (translation.isLoading.value) {
        translation.cancel()
      }
      return
    }

    if (translation.failure.value) {
      translation.clearFailure()
    }
    if (translation.isLoading.value || translation.status.value === 'ready') {
      translation.cancel()
    }
  }

  watch(
    [translation.status, translation.targetLanguage, translation.failure],
    () => options.onMenuInvalidated?.(),
  )

  return {
    status: translation.status,
    failure: translation.failure,
    targetLanguage: translation.targetLanguage,
    isLoading: translation.isLoading,
    ensureCatalog,
    evaluateAuto,
    getManualTargets,
    requestManual,
    resetForSource,
    resetForPreferences,
    resetForPlayer,
    handlePlayerState,
    clearFailure: translation.clearFailure,
    cancel: () => {
      manualRequestId += 1
      translation.cancel()
    },
  }
}

/**
 * Normalizes one language code for the two-letter translation contract.
 *
 * @param value Raw language code.
 * @returns Lowercase two-letter code, or `null`.
 */
function toTranslationCode(value: unknown): string | null {
  const base = getBaseLanguageCode(normalizeLanguageCode(value))
  return base && /^[a-z]{2}$/.test(base) ? base : null
}

/**
 * Checks whether a language list contains one base language.
 *
 * @param values Catalog language list.
 * @param language Two-letter language code.
 * @returns True when the base language appears in the list.
 */
function languageMatches(values: readonly string[], language: string): boolean {
  const target = toTranslationCode(language)
  return target !== null && values.some((value) => toTranslationCode(value) === target)
}

/**
 * Converts a source link to the absolute HTTP URL accepted by the backend.
 *
 * @param value Source link.
 * @returns Absolute URL, or `null` for unsupported schemes.
 */
function toAbsoluteHttpUrl(value: string): string | null {
  try {
    const url = new URL(value, window.location.href)
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null
  } catch {
    return null
  }
}

/**
 * Builds a stable key for one automatic request attempt.
 *
 * @param url Source subtitle URL.
 * @param sourceLanguage Known source language, or `null`.
 * @param targetLanguage Target language.
 * @returns Request key scoped to the active source.
 */
function buildRequestKey(
  url: string,
  sourceLanguage: string | null,
  targetLanguage: string,
): string {
  return `${url}\u0000${sourceLanguage ?? ''}\u0000${targetLanguage}`
}
