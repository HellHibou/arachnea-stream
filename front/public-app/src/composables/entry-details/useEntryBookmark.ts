import { computed, shallowRef, watch, type Ref } from 'vue'
import type { MediaItem } from '@/types/media'
import type { EntryBookmark, useStorage } from '@/services/storage'
import type { entryDetailsData } from './entryDetailsData'
import type { entryEpisodeSelection } from './entryEpisodeSelection'
import type { entryDetailsPresentation } from './entryDetailsPresentation'

interface EntryBookmarkOptions {
  source: Ref<string>
  entry: Ref<string>
  storage: ReturnType<typeof useStorage>
}

interface BookmarkSelectionOptions {
  data: Pick<ReturnType<typeof entryDetailsData>, 'details' | 'selectedSeasonId'>
  selection: Pick<ReturnType<typeof entryEpisodeSelection>, 'displayedEpisodes' | 'selectedEpisodeId' | 'selectEpisodeById'>
  presentation: Pick<ReturnType<typeof entryDetailsPresentation>, 'selectedSeasonLabel'>
  hasNextEpisode: Ref<boolean>
}

/**
 * Owns bookmark preferences and playback progress for one entry.
 * @param options Reactive entry identity and the existing storage instance.
 * @returns Preferences available before data loading, reset operations and selection binding.
 */
export function useEntryBookmark(options: EntryBookmarkOptions) {
  const { source, entry, storage } = options
  /** Latest playback time from the media player. */
  const latestPlaybackTime = shallowRef<number | null>(null)

  /** Latest playback duration in seconds reported by the media player. */
  const latestPlaybackDuration = shallowRef<number | null>(null)

  /** Key to track restored episode bookmark state. */
  const restoredEpisodeBookmarkKey = shallowRef<string | null>(null)

  /**
   * Returns the bookmark currently stored for the active entry details page.
   */
  const currentEntryBookmark = computed(() =>
    storage.getEntryBookmark(source.value, entry.value),
  )

  /**
   * Indicates whether the active entry is currently bookmarked.
   */
  const isBookmarked = computed(() => currentEntryBookmark.value !== null)

  /**
   * Exposes the preferred season identifier restored from local storage when available.
   */
  const preferredSeasonId = computed(() => currentEntryBookmark.value?.groupId ?? null)

  /**
   * Exposes the preferred episode identifier restored from local storage when available.
   */
  const preferredEpisodeId = computed(() => currentEntryBookmark.value?.selectedItemId ?? null)

  /**
   * Stores one bookmark payload for the current entry.
   *
   * @param bookmark Bookmark payload to persist.
   */
  function saveEntryBookmark(bookmark: EntryBookmark) {
    storage.setEntryBookmark(source.value, entry.value, bookmark)
  }

  /** Clears the progress belonging to the previously selected episode. */
  function resetPlayback() {
    latestPlaybackTime.value = null
    latestPlaybackDuration.value = null
  }

  /** Clears progress and the restoration guard when the entry identity changes. */
  function resetEntry() {
    resetPlayback()
    restoredEpisodeBookmarkKey.value = null
  }

  /**
   * Persists the season selection while retaining the entry's existing bookmark snapshot.
   * @param item Newly selected season card.
   */
  function saveSeasonSelection(item: MediaItem) {
    if (!isBookmarked.value) {
      return
    }
    const previousBookmark = currentEntryBookmark.value
    saveEntryBookmark({
      groupId: item.id,
      selectedItemId: null,
      playbackTime: null,
      title: previousBookmark?.title ?? null,
      alternativeTitleLabel: previousBookmark?.alternativeTitleLabel ?? null,
      imagePosterUrl: previousBookmark?.imagePosterUrl ?? null,
      imageLandscapeUrl: previousBookmark?.imageLandscapeUrl ?? null,
      imageUrl: previousBookmark?.imageUrl ?? null,
      description: previousBookmark?.description ?? null,
      mediaTypeLabel: previousBookmark?.mediaTypeLabel ?? null,
      seasonLabel: item.title?.trim() || null,
      episodeNumber: null,
      isFullyWatched: false,
    })
  }

  /**
   * Binds loaded selection state once, after navigation resets have been registered.
   * @param context Loaded data, selection, presentation and next-episode availability.
   * @returns Bookmark save/toggle and progress handlers bound to those reactive refs.
   */
  function bindSelection(context: BookmarkSelectionOptions) {
    const { details, selectedSeasonId } = context.data
    const { displayedEpisodes, selectedEpisodeId, selectEpisodeById } = context.selection
    const { selectedSeasonLabel } = context.presentation
    const { hasNextEpisode } = context
    /** Restores episode selection from bookmark when episodes or source changes. */
    watch(
      [displayedEpisodes, preferredEpisodeId, source, entry],
      ([episodes, nextEpisodeId, currentSource, currentEntry]) => {
        if (!nextEpisodeId) {
          return
        }

        const restoreKey = JSON.stringify([currentSource, currentEntry, nextEpisodeId])
        if (restoredEpisodeBookmarkKey.value === restoreKey) {
          return
        }

        if (selectedEpisodeId.value === nextEpisodeId) {
          restoredEpisodeBookmarkKey.value = restoreKey
          return
        }

        if (!episodes.some((episode) => episode.id === nextEpisodeId)) {
          return
        }

        if (!selectEpisodeById(nextEpisodeId)) {
          return
        }

        restoredEpisodeBookmarkKey.value = restoreKey
      },
      { immediate: true },
    )

    /**
     * Tolerance in seconds applied when detecting that playback reached the end of an episode.
     */
    const PLAYBACK_END_TOLERANCE_SECONDS = 1

    /**
     * Indicates whether the current playback position has reached the end of the episode.
     */
    const isCurrentEpisodeAtEnd = computed(() => {
      const playbackTime = latestPlaybackTime.value
      const duration = latestPlaybackDuration.value

      return (
        playbackTime !== null &&
        duration !== null &&
        duration > 0 &&
        playbackTime >= duration - PLAYBACK_END_TOLERANCE_SECONDS
      )
    })

    /**
     * Exposes the 1-based number of the selected episode within the displayed list.
     */
    const selectedEpisodeNumber = computed(() => {
      if (!selectedEpisodeId.value) {
        return null
      }

      const index = displayedEpisodes.value.findIndex(
        (episode) => episode.id === selectedEpisodeId.value,
      )

      return index >= 0 ? index + 1 : null
    })

    /**
     * Stores the bookmark using the current selection state and entry details snapshot.
     *
     * @param playbackTime Playback position in seconds to persist for the active entry.
     */
    function saveCurrentBookmark(playbackTime: number | null = latestPlaybackTime.value) {
      saveEntryBookmark({
        groupId: selectedSeasonId.value,
        selectedItemId: selectedEpisodeId.value,
        playbackTime,
        title: details.value?.title ?? null,
        alternativeTitleLabel: details.value?.alternativeTitleLabel ?? null,
        imagePosterUrl: details.value?.imagePosterUrl ?? null,
        imageLandscapeUrl: details.value?.imageLandscapeUrl ?? null,
        imageUrl: details.value?.imageUrl ?? null,
        description: details.value?.description ?? null,
        mediaTypeLabel: null,
        seasonLabel: selectedSeasonLabel.value ?? null,
        episodeNumber: selectedEpisodeNumber.value,
        isFullyWatched: !hasNextEpisode.value && isCurrentEpisodeAtEnd.value,
      })
    }

    /**
     * Toggles the bookmark for the active entry details page.
     */
    function handleBookmarkToggle() {
      if (isBookmarked.value) {
        storage.removeEntryBookmark(source.value, entry.value)
        return
      }

      saveCurrentBookmark()
    }

    /**
     * Stores the latest playback position and duration emitted by the integrated
     * Video.js renderer, then persists the bookmark for the active entry.
     *
     * @param playbackTime Playback position in seconds, or `null` when it should be cleared.
     * @param duration Total media duration in seconds, or `null` when unavailable.
     */
    function handlePlaybackProgressUpdate(playbackTime: number | null, duration: number | null) {
      latestPlaybackTime.value = playbackTime
      latestPlaybackDuration.value = duration

      if (!isBookmarked.value) {
        return
      }

      saveCurrentBookmark(playbackTime)
    }

    return { saveCurrentBookmark, handleBookmarkToggle, handlePlaybackProgressUpdate }
  }

  return {
    currentEntryBookmark, isBookmarked, preferredSeasonId, preferredEpisodeId,
    latestPlaybackTime, resetPlayback, resetEntry, saveSeasonSelection, bindSelection,
  }
}

export type EntryBookmarkController = ReturnType<typeof useEntryBookmark>
