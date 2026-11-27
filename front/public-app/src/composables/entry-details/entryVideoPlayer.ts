import { computed, shallowRef, watch } from 'vue'
import { t } from '@/i18n'
import { resolvePlayerMediaSource } from '@/services/players'
import type { EntryDetails } from '@/types/entry'
import type { UseEntryVideoPlayerOptions } from './entryPlayerTypes'
import { createEntryPlayerSelection } from './entryPlayerSelection'
import { createEntryPlayerStream } from './entryPlayerStream'
import { createEntryPlayerTrailer } from './entryPlayerTrailer'

export type { EntryPlayerLanguageOption } from './entryPlayerTypes'

/**
 * Composes selection, primary stream and trailer state for one entry details view.
 * @param options Reactive entry/item data and security/background preferences.
 * @returns The existing player view model consumed by entry detail pages.
 */
export function entryVideoPlayer(options: UseEntryVideoPlayerOptions) {
  const activeVideoMode = shallowRef<'media' | 'trailer'>('media')
  const selection = createEntryPlayerSelection({
    details: options.details, selectedPlayableItem: options.selectedPlayableItem,
    activateMedia: () => { activeVideoMode.value = 'media' },
  })
  const {
    activeLanguageKey, activePlayerId, availableLanguages, filteredPlayers, selectedPlayer,
    selectedPlayableTitle, showLanguageSelector, showPlayerSelector, showPlayerControls, activePlayers,
    rememberCurrentLanguage, rememberCurrentPlayer,
  } = selection
  const streams = createEntryPlayerStream({
    details: options.details, selectedPlayableItem: options.selectedPlayableItem, selectedPlayer,
    selectNextPlayer: selection.selectNextPlayer,
    acceptResolvedPlayer: selection.acceptResolvedPlayer,
  })
  const {
    mediaSource, mediaOpenUrl, hasMediaCandidate, resolvedPlayerPosterUrl,
    isMediaPlayerLoading, mediaPlayerErrorMessage, loadDeferredPlayers, handleMediaSourceError: handleStreamSourceError,
  } = streams
  const trailers = createEntryPlayerTrailer({
    details: options.details, securityMode: options.securityMode,
    useTrailerAsBackground: options.useTrailerAsBackground, activeVideoMode,
  })
  const { trailerUrl, trailerMediaSource, hasTrailer, isTrailerBlocked } = trailers

  /**
   * Keeps a selected episode's surface visible while its players resolve, including bookmark restoration.
   *
   * @returns True for a selected episode, or entry players without a trailer.
   */
  const shouldKeepMediaSurfaceVisible = computed(() =>
    Boolean(options.selectedPlayableItem.value) ||
    (activePlayers.value.length > 0 && !hasTrailer.value),
  )

  /**
   * Exposes whether the built-in trailer player should be rendered.
   *
   * @returns True when a trailer is available and should be shown.
   */
  const showTrailerPlayer = computed(() =>
    hasTrailer.value &&
    !isTrailerBlocked.value &&
    (activeVideoMode.value === 'trailer' || (!options.selectedPlayableItem.value && !mediaSource.value)),
  )

  /**
   * Exposes whether the built-in media player should be rendered.
   *
   * @returns True when in media mode and a candidate, loading state, or error exists.
   */
  const showMediaPlayer = computed(() =>
    activeVideoMode.value !== 'trailer' &&
    (
      hasMediaCandidate.value ||
      shouldKeepMediaSurfaceVisible.value ||
      isMediaPlayerLoading.value ||
      Boolean(mediaPlayerErrorMessage.value)
    ),
  )

  /**
   * Exposes whether the trailer toggle action should be shown below the cover.
   *
   * @returns True when a trailer is available and not blocked in `safe` mode.
   */
  const showTrailerAction = computed(() =>
    hasTrailer.value && !isTrailerBlocked.value,
  )

  /**
   * Exposes the trailer action label depending on the current player mode.
   *
   * @returns The label for the trailer toggle action.
   */
  const trailerActionLabel = computed(() =>
    activeVideoMode.value === 'trailer' && hasMediaCandidate.value ? t('entry.video') : t('entry.trailer'),
  )

  /**
   * Toggles the built-in player between the trailer and the current media video.
   */
  function handleTrailerToggle(): void {
    if (!hasTrailer.value) {
      return
    }

    if (hasMediaCandidate.value) {
      activeVideoMode.value = activeVideoMode.value === 'trailer' ? 'media' : 'trailer'
      return
    }

    activeVideoMode.value = 'trailer'
  }

  /** Advances servers only for errors from the primary media surface. */
  function handleMediaSourceError(): void {
    if (activeVideoMode.value === 'media') handleStreamSourceError()
  }

  /**
   * Forces the built-in player back to media mode after an explicit selection.
   */
  function activateMediaPlayer(): void {
    activeVideoMode.value = 'media'
    void loadDeferredPlayers()
  }

  /** Clears controller-owned entry state before loading another entry. */
  function resetForEntryLoad() {
    streams.resetForEntryLoad()
    trailers.resetForEntryLoad()
    activeVideoMode.value = 'media'
    selection.resetForEntryLoad()
  }

  /**
   * Applies the initial player state for one freshly loaded entry.
   *
   * @param nextDetails - Detailed entry returned by the backend.
   */
  function applyLoadedEntryDefaults(nextDetails: EntryDetails): void {
    activeVideoMode.value = options.selectedPlayableItem.value || nextDetails.players.entries.length > 0
      ? 'media'
      : (resolvePlayerMediaSource(nextDetails.trailerUrl) || nextDetails.trailerResolver ? 'trailer' : 'media')
    selection.applyLoadedEntryDefaults()
  }

  watch(options.details, (nextDetails) => {
    if (!nextDetails) {
      resetForEntryLoad()
      return
    }

    applyLoadedEntryDefaults(nextDetails)
  }, { immediate: true })
  // Initialize episode choices before selector reconciliation and stream resolution.
  selection.observePlayableItem()
  selection.observeSelectors()
  trailers.observe()
  streams.observe()
  watch(activeLanguageKey, (nextLanguageKey, previousLanguageKey) => {
    if (
      previousLanguageKey !== undefined &&
      nextLanguageKey !== previousLanguageKey &&
      hasMediaCandidate.value
    ) {
      activeVideoMode.value = 'media'
    }
  })

  watch(activePlayerId, (nextPlayerId, previousPlayerId) => {
    if (
      previousPlayerId !== undefined &&
      nextPlayerId !== previousPlayerId &&
      hasMediaCandidate.value
    ) {
      activeVideoMode.value = 'media'
    }
  })

  watch(mediaSource, (nextMediaSource) => {
    if (nextMediaSource && options.selectedPlayableItem.value) {
      activeVideoMode.value = 'media'
      return
    }

    if (!nextMediaSource && !options.selectedPlayableItem.value && !hasMediaCandidate.value && hasTrailer.value) {
      activeVideoMode.value = 'trailer'
    }
  })

  return {
    activeLanguageKey,
    availableLanguages,
    filteredPlayers,
    activePlayerId,
    mediaSource,
    mediaOpenUrl,
    isMediaPlayerLoading,
    mediaPlayerErrorMessage,
    resolvedPlayerPosterUrl,
    trailerUrl,
    trailerMediaSource,
    showTrailerPlayer,
    selectedPlayableTitle,
    showMediaPlayer,
    showTrailerAction,
    trailerActionLabel,
    showLanguageSelector,
    showPlayerSelector,
    showPlayerControls,
    activateMediaPlayer,
    handleTrailerToggle,
    rememberCurrentLanguage,
    rememberCurrentPlayer,
    handleMediaSourceError,
  }
}
