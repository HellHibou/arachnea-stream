import { computed, shallowRef, watch, type Ref } from 'vue'
import type { EntryEpisode } from '@/types/entry'
import type { MediaItem } from '@/types/media'
import type { Parameters } from '@/services/storage'
import { getSeasonEpisodes } from '@/services/rustify'
import type { entryDetailsData } from './entryDetailsData'
import type { entryEpisodeSelection } from './entryEpisodeSelection'
import type { entryVideoPlayer } from './entryVideoPlayer'
import type { EntryBookmarkController } from './useEntryBookmark'

interface EntryEpisodeNavigationOptions {
  source: Ref<string>
  entry: Ref<string>
  webUrl: Ref<string | null>
  data: Pick<ReturnType<typeof entryDetailsData>, 'details' | 'selectedSeasonId' | 'selectSeasonById' | 'hasMoreSeasonEpisodes' | 'handleLoadMoreSeasonEpisodes' | 'handleSeasonSelect'>
  selection: Pick<ReturnType<typeof entryEpisodeSelection>, 'selectedEpisodeId' | 'hasPreviousEpisode' | 'hasNextEpisode' | 'selectEpisodeById' | 'handleEpisodeSelect' | 'handleEpisodeStep' | 'resetSelectedEpisode'>
  player: Pick<ReturnType<typeof entryVideoPlayer>, 'mediaSource' | 'mediaPlayerErrorMessage' | 'activateMediaPlayer' | 'handleTrailerToggle' | 'activeLanguageKey' | 'activePlayerId'>
  bookmark: Pick<EntryBookmarkController, 'isBookmarked' | 'latestPlaybackTime' | 'resetPlayback' | 'resetEntry' | 'saveSeasonSelection'>
  parameters: Parameters
  saveCurrentBookmark: (playbackTime?: number | null) => void
}

/**
 * Owns episode transitions, cross-season navigation and cancellation state.
 * @param options Reactive feature outputs and bookmark operations used by navigation.
 * @returns Transition flags, next-episode availability and player/navigation handlers.
 */
export function useEntryEpisodeNavigation(options: EntryEpisodeNavigationOptions) {
  const { source, entry, webUrl, bookmark, parameters, saveCurrentBookmark } = options
  const { details, selectedSeasonId, selectSeasonById, hasMoreSeasonEpisodes, handleLoadMoreSeasonEpisodes,
    handleSeasonSelect: handleDataSeasonSelect } = options.data
  const { selectedEpisodeId, hasPreviousEpisode, hasNextEpisode: hasNextEpisodeInSeason, selectEpisodeById,
    handleEpisodeSelect, handleEpisodeStep, resetSelectedEpisode } = options.selection
  const { mediaSource, mediaPlayerErrorMessage, activateMediaPlayer, handleTrailerToggle, activeLanguageKey, activePlayerId } = options.player
  const { isBookmarked, latestPlaybackTime } = bookmark

  /** Whether episode autoplay is pending. */
  const pendingEpisodeAutoplay = shallowRef(false)

  /** Source URL for pending episode autoplay. */
  const pendingEpisodeAutoplaySourceUrl = shallowRef<string | null>(null)

  /** Whether the media surface should remain mounted during episode transition. */
  const shouldKeepMediaSurfaceMounted = shallowRef(false)

  /** Whether episode navigation is currently in progress. */
  const isEpisodeNavigationInProgress = shallowRef(false)

  /** Counter to track the latest episode navigation request ID. */
  let latestEpisodeNavigationId = 0

  /**
   * Indicates whether another season could provide a playable next episode.
   */
  const hasLaterSeason = computed(() => {
    const seasons = details.value?.seasons ?? []

    if (!selectedSeasonId.value) {
      return false
    }

    const currentSeasonIndex = seasons.findIndex((season) => season.id === selectedSeasonId.value)
    if (currentSeasonIndex < 0) {
      return false
    }

    return seasons.slice(currentSeasonIndex + 1).some((season) =>
      season.episodes.length > 0 || Boolean(season.link),
    )
  })

  /**
   * Keeps the next-episode navigation enabled when another season is available.
   */
  const hasNextEpisode = computed(() =>
    hasNextEpisodeInSeason.value || hasLaterSeason.value,
  )

  /**
   * Keeps the current media surface mounted from the beginning of one episode transition.
   *
   * @param autoplay Indicates whether the next selected episode should start automatically.
   */
  function beginEpisodeSelectionTransition(autoplay: boolean) {
    const shouldCaptureAutoplaySourceUrl = autoplay && !pendingEpisodeAutoplay.value

    shouldKeepMediaSurfaceMounted.value = true
    pendingEpisodeAutoplay.value = autoplay

    if (!autoplay) {
      pendingEpisodeAutoplaySourceUrl.value = null
      return
    }

    if (shouldCaptureAutoplaySourceUrl) {
      pendingEpisodeAutoplaySourceUrl.value = mediaSource.value?.src ?? null
    }
  }

  /**
   * Cancels any in-flight automatic episode navigation request.
   */
  function cancelPendingEpisodeNavigation() {
    latestEpisodeNavigationId += 1
    pendingEpisodeAutoplay.value = false
    pendingEpisodeAutoplaySourceUrl.value = null
    shouldKeepMediaSurfaceMounted.value = false
    isEpisodeNavigationInProgress.value = false
  }

  /**
   * Returns the first playable episode from one season page payload.
   *
   * @param episodes Episodes returned for one season page.
   * @returns First playable episode or `null` when none is available.
   */
  function findFirstPlayableEpisode(episodes: EntryEpisode[]): EntryEpisode | null {
    return episodes.find(
      (episode) => episode.players.entries.length > 0 || Boolean(episode.players.link),
    ) ?? null
  }

  /**
   * Applies the side effects shared by manual and automatic episode navigation.
   *
   * @param autoplay Indicates whether the next selected episode should start automatically.
   */
  function applyEpisodeSelectionEffects(autoplay: boolean) {
    beginEpisodeSelectionTransition(autoplay)
    activateMediaPlayer()
    bookmark.resetPlayback()

    if (!isBookmarked.value) {
      return
    }

    saveCurrentBookmark(null)
  }

  /**
   * Loads a season in the background until one playable episode is discovered.
   *
   * @param seasonId Identifier of the season to inspect.
   * @returns First playable episode found in that season, or `null` when it stays empty.
   */
  async function findFirstPlayableEpisodeInSeason(seasonId: string): Promise<EntryEpisode | null> {
    const season = (details.value?.seasons ?? []).find((entrySeason) => entrySeason.id === seasonId)

    if (!season) {
      return null
    }

    // Check embedded episodes first.
    if (season.episodes.length > 0) {
      return findFirstPlayableEpisode(season.episodes)
    }

    if (!season.link) {
      return null
    }

    let nextPage = 1

    while (true) {
      const seasonPage = await getSeasonEpisodes(source.value, season.link, nextPage, season.label)
      const firstPlayableEpisode = findFirstPlayableEpisode(seasonPage.episodes)

      if (firstPlayableEpisode) {
        return firstPlayableEpisode
      }

      if (!seasonPage.haveMore || seasonPage.currentPage < nextPage) {
        return null
      }

      nextPage = seasonPage.currentPage + 1
    }
  }

  /**
   * Loads the requested season into the visible state until the target episode becomes selectable.
   *
   * @param seasonId Identifier of the season that should become active.
   * @param episodeId Identifier of the episode that should become active once loaded.
   * @returns `true` when the episode could be selected from the visible season list.
   */
  async function loadSeasonEpisodeIntoVisibleState(
    seasonId: string,
    episodeId: string,
  ): Promise<boolean> {
    const seasonLoaded = await selectSeasonById(seasonId)

    if (!seasonLoaded) {
      return false
    }

    if (selectEpisodeById(episodeId)) {
      return true
    }

    while (hasMoreSeasonEpisodes.value) {
      await handleLoadMoreSeasonEpisodes()

      if (selectEpisodeById(episodeId)) {
        return true
      }
    }

    return false
  }

  /**
   * Moves to the next playable episode, loading the next season when needed.
   *
   * @param autoplay Indicates whether the newly selected episode should start automatically.
   * @returns `true` when a next playable episode was selected.
   */
  async function selectNextPlayableEpisode(autoplay: boolean): Promise<boolean> {
    if (isEpisodeNavigationInProgress.value) {
      return false
    }

    beginEpisodeSelectionTransition(autoplay)

    const navigationId = latestEpisodeNavigationId + 1
    latestEpisodeNavigationId = navigationId
    isEpisodeNavigationInProgress.value = true

    try {
      if (hasNextEpisodeInSeason.value) {
        if (navigationId !== latestEpisodeNavigationId) {
          return false
        }

        handleEpisodeStep(1)
        applyEpisodeSelectionEffects(autoplay)
        return true
      }

      const seasons = details.value?.seasons ?? []
      const currentSeasonIndex = seasons.findIndex((season) => season.id === selectedSeasonId.value)

      if (currentSeasonIndex < 0) {
        pendingEpisodeAutoplay.value = false
        pendingEpisodeAutoplaySourceUrl.value = null
        shouldKeepMediaSurfaceMounted.value = false
        return false
      }

      for (const season of seasons.slice(currentSeasonIndex + 1)) {
        if (!season.link) {
          continue
        }

        const firstPlayableEpisode = await findFirstPlayableEpisodeInSeason(season.id)
        if (navigationId !== latestEpisodeNavigationId) {
          return false
        }

        if (!firstPlayableEpisode) {
          continue
        }

        const episodeSelected = await loadSeasonEpisodeIntoVisibleState(
          season.id,
          firstPlayableEpisode.id,
        )
        if (navigationId !== latestEpisodeNavigationId) {
          return false
        }

        if (!episodeSelected) {
          pendingEpisodeAutoplay.value = false
          pendingEpisodeAutoplaySourceUrl.value = null
          continue
        }

        applyEpisodeSelectionEffects(autoplay)
        return true
      }

      pendingEpisodeAutoplay.value = false
      pendingEpisodeAutoplaySourceUrl.value = null
      shouldKeepMediaSurfaceMounted.value = false
      return false
    } catch (error) {
      pendingEpisodeAutoplay.value = false
      pendingEpisodeAutoplaySourceUrl.value = null
      shouldKeepMediaSurfaceMounted.value = false
      console.error('Failed to select the next playable episode:', error)
      return false
    } finally {
      if (navigationId === latestEpisodeNavigationId) {
        isEpisodeNavigationInProgress.value = false
      }
    }
  }

  /**
   * Selects one episode and forces the player back to media mode.
   *
   * @param item Episode card selected from the episode collection.
   */
  function handlePlayerEpisodeSelect(item: MediaItem) {
    cancelPendingEpisodeNavigation()
    const previousEpisodeId = selectedEpisodeId.value
    beginEpisodeSelectionTransition(false)
    handleEpisodeSelect(item)

    if (selectedEpisodeId.value === previousEpisodeId) {
      shouldKeepMediaSurfaceMounted.value = false
      return
    }

    applyEpisodeSelectionEffects(false)
  }

  /**
   * Moves to the adjacent playable episode and keeps the player in media mode.
   *
   * @param offset Relative episode offset applied from the hero controls.
   */
  async function handlePlayerEpisodeStep(offset: -1 | 1) {
    if (offset < 0) {
      if (!hasPreviousEpisode.value) {
        return
      }

      cancelPendingEpisodeNavigation()
      beginEpisodeSelectionTransition(false)
      handleEpisodeStep(offset)
      applyEpisodeSelectionEffects(false)
      return
    }

    await selectNextPlayableEpisode(false)
  }

  /**
   * Moves to the previous or next playable episode triggered by the player control bar navigation
   * buttons, and starts playback automatically.
   *
   * @param offset Relative episode offset to apply.
   */
  async function handlePlayerControlStep(offset: -1 | 1) {
    if (offset < 0) {
      if (!hasPreviousEpisode.value) {
        return
      }

      cancelPendingEpisodeNavigation()
      beginEpisodeSelectionTransition(true)
      handleEpisodeStep(offset)
      applyEpisodeSelectionEffects(true)
      return
    }

    await selectNextPlayableEpisode(true)
  }

  /**
   * Cancels automatic navigation before toggling between trailer and media.
   */
  function handlePlayerTrailerToggle() {
    cancelPendingEpisodeNavigation()
    handleTrailerToggle()
  }

  /**
   * Stores the selected language and cancels any pending automatic episode transition.
   *
   * @param value Language key selected in the integrated player.
   */
  function handleActiveLanguageKeyUpdate(value: string | null) {
    cancelPendingEpisodeNavigation()
    activeLanguageKey.value = value
  }

  /**
   * Stores the selected player and cancels any pending automatic episode transition.
   *
   * @param value Player identifier selected in the integrated player.
   */
  function handleActivePlayerIdUpdate(value: string | null) {
    cancelPendingEpisodeNavigation()
    activePlayerId.value = value
  }

  /**
   * Stores the episode autoplay preference and cancels any pending automatic transition.
   *
   * @param value Indicates whether episode autoplay should stay enabled.
   */
  function handleEpisodeAutoplayPreferenceUpdate(value: boolean) {
    cancelPendingEpisodeNavigation()
    parameters.isEpisodeAutoplayEnabled.value = value
  }

  /**
   * Clears the pending autoplay flag once the next episode has effectively started.
   *
   * @param playbackSourceUrl Source URL that emitted the playback start event.
   */
  function handleMediaPlaybackStarted(playbackSourceUrl: string | null) {
    if (
      pendingEpisodeAutoplay.value &&
      playbackSourceUrl &&
      playbackSourceUrl === pendingEpisodeAutoplaySourceUrl.value
    ) {
      return
    }

    pendingEpisodeAutoplay.value = false
    pendingEpisodeAutoplaySourceUrl.value = null
    shouldKeepMediaSurfaceMounted.value = false
  }

  /**
   * Marks the bookmark as fully watched when the last playable episode ends,
   * then starts the next playable episode automatically when the preference is enabled.
   */
  async function handleMediaPlaybackEnded() {
    if (isBookmarked.value && !hasNextEpisode.value) {
      saveCurrentBookmark(latestPlaybackTime.value)
    }

    if (!parameters.isEpisodeAutoplayEnabled.value) {
      return
    }

    await selectNextPlayableEpisode(true)
  }

  /**
   * Loads a new season without affecting the current player state.
   * @param item Season card selected from the season collection.
   */
  function handleSeasonSelect(item: MediaItem) {
    cancelPendingEpisodeNavigation()
    bookmark.resetPlayback()
    void handleDataSeasonSelect(item)
    bookmark.saveSeasonSelection(item)
  }

  /** Resets entry state when source, entry, or webUrl props change. */
  watch([source, entry, webUrl], () => {
    bookmark.resetEntry()
    cancelPendingEpisodeNavigation()
    resetSelectedEpisode()
  }, { immediate: true })

  /** Resets media surface mounted state when media source or error changes. */
  watch(
    [() => mediaSource.value?.src ?? null, mediaPlayerErrorMessage],
    ([nextMediaSourceUrl, nextMediaPlayerErrorMessage]) => {
      if (!nextMediaSourceUrl && !nextMediaPlayerErrorMessage) {
        return
      }

      shouldKeepMediaSurfaceMounted.value = false
    },
  )

  return {
    pendingEpisodeAutoplay, shouldKeepMediaSurfaceMounted, hasNextEpisode,
    handleSeasonSelect, handlePlayerEpisodeSelect, handlePlayerEpisodeStep, handlePlayerControlStep,
    handlePlayerTrailerToggle, handleActiveLanguageKeyUpdate, handleActivePlayerIdUpdate,
    handleEpisodeAutoplayPreferenceUpdate, handleMediaPlaybackStarted, handleMediaPlaybackEnded,
  }
}
