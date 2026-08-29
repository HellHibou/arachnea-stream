import { computed, ref, shallowRef, watch, type Ref } from 'vue'

import { getEntryDetails, getSeasonEpisodes } from '@/services/rustify'
import { t } from '@/i18n'
import type { EntryDetails, EntryEpisode, EntrySeason } from '@/types/entry'
import type { MediaItem } from '@/types/media'

/**
 * Options accepted by the entry details data controller.
 */
interface UseEntryDetailsDataOptions {
  /**
   * Backend source used to resolve the current entry.
   */
  source: Ref<string>
  /**
   * Absolute entry URL passed to the backend `get_entry` function.
   */
  entry: Ref<string>
  /**
   * Public web URL associated with the selected entry.
   */
  webUrl: Ref<string | null>
  /**
   * Preferred season identifier restored from local storage when available.
   */
  preferredSeasonId: Ref<string | null>
}

/**
 * Owns backend loading, season pagination, and loading/error state for one entry details view.
 *
 * @param options Reactive entry identifiers used to keep the data state in sync with the current details request.
 * @returns Entry details data state and actions for seasons.
 */
export function entryDetailsData(options: UseEntryDetailsDataOptions) {
  /** Reactive reference to the loaded entry details, or null if not yet loaded. */
  const details = ref<EntryDetails | null>(null)
  /** Whether the entry details are currently being loaded. */
  const isLoading = shallowRef(false)
  /** Error message from entry details loading, or null if successful. */
  const errorMessage = shallowRef<string | null>(null)
  /** Reactive array of episodes for the currently selected season. */
  const seasonEpisodes = ref<EntryEpisode[]>([])
  /** The currently selected season identifier. */
  const selectedSeasonId = shallowRef<string | null>(null)
  /** Whether season episodes are currently being loaded. */
  const isSeasonLoading = shallowRef(false)
  /** Whether additional season episodes are currently being loaded. */
  const isSeasonLoadingMore = shallowRef(false)
  /** Error message from season episodes loading, or null if successful. */
  const seasonErrorMessage = shallowRef<string | null>(null)
  /** The current page number for season episodes pagination. */
  const currentSeasonPage = shallowRef(1)
  /** Whether there are more season episodes available to load. */
  const hasMoreSeasonEpisodes = shallowRef(false)

  /** Request identifier counter for entry details requests to ignore stale responses. */
  let latestRequestId = 0
  /** Request identifier counter for season requests to ignore stale responses. */
  let latestSeasonRequestId = 0

  /**
   * Resolves the currently selected season from the fetched entry details.
   *
   * @returns The selected season object, or null if not found.
   */
  const selectedSeason = computed<EntrySeason | null>(() =>
    (details.value?.seasons ?? []).find((season) => season.id === selectedSeasonId.value) ?? null,
  )

  /**
   * Clears the season-specific state whenever the featured entry changes.
   * Resets all season-related reactive state to initial values.
   */
  function resetSeasonState(): void {
    latestSeasonRequestId += 1
    selectedSeasonId.value = null
    seasonEpisodes.value = []
    seasonErrorMessage.value = null
    isSeasonLoading.value = false
    isSeasonLoadingMore.value = false
    currentSeasonPage.value = 1
    hasMoreSeasonEpisodes.value = false
  }

  /**
   * Loads one season page from the backend or from embedded episodes and updates the local state.
   *
   * When the season has embedded episodes, they are used directly without a network request.
   * Otherwise the backend `get_season` endpoint is called for dynamic/lazy loading.
   *
   * @param season Season currently being fetched.
   * @param page Requested page number.
   * @param append Indicates whether the fetched episodes should be appended to the current list.
   */
  async function loadSeasonPage(season: EntrySeason, page: number, append: boolean) {
    const requestId = ++latestSeasonRequestId
    selectedSeasonId.value = season.id
    seasonErrorMessage.value = null

    if (append) {
      isSeasonLoadingMore.value = true
    } else {
      seasonEpisodes.value = []
      isSeasonLoading.value = true
      isSeasonLoadingMore.value = false
      currentSeasonPage.value = 1
      hasMoreSeasonEpisodes.value = false
    }

    // Embedded episodes take precedence: no network call needed.
    if (season.episodes.length > 0) {
      if (requestId !== latestSeasonRequestId) {
        return
      }

      seasonEpisodes.value = season.episodes
      currentSeasonPage.value = 1
      hasMoreSeasonEpisodes.value = false
      isSeasonLoading.value = false
      isSeasonLoadingMore.value = false
      return
    }

    if (!season.link) {
      if (requestId !== latestSeasonRequestId) {
        return
      }

      seasonEpisodes.value = []
      isSeasonLoading.value = false
      isSeasonLoadingMore.value = false
      return
    }

    try {
      const nextPage = await getSeasonEpisodes(options.source.value, season.link, page, season.label)

      if (requestId !== latestSeasonRequestId) {
        return
      }

      seasonEpisodes.value = append
        ? seasonEpisodes.value.concat(nextPage.episodes)
        : nextPage.episodes
      currentSeasonPage.value = nextPage.currentPage
      hasMoreSeasonEpisodes.value = nextPage.haveMore
    } catch (error) {
      if (requestId !== latestSeasonRequestId) {
        return
      }

      if (!append) {
        seasonEpisodes.value = []
      }

      seasonErrorMessage.value =
        error instanceof Error ? error.message : t('errors.season')
    } finally {
      if (requestId === latestSeasonRequestId) {
        if (append) {
          isSeasonLoadingMore.value = false
        } else {
          isSeasonLoading.value = false
        }
      }
    }
  }

  /**
   * Loads the first page of episodes for the requested season.
   *
   * @param item Season card selected from the season collection.
   */
  async function handleSeasonSelect(item: MediaItem) {
    const season = (details.value?.seasons ?? []).find((entry) => entry.id === item.id)

    if (!season) {
      return
    }

    await loadSeasonPage(season, 1, false)
  }

  /**
   * Loads the stored season when reopening an entry details page.
   *
   * @param seasonId Preferred season identifier persisted for the current entry.
   * @returns `true` when the preferred season could be loaded.
   */
  async function selectSeasonById(seasonId: string | null): Promise<boolean> {
    if (!seasonId) {
      return false
    }

    const season = (details.value?.seasons ?? []).find((entry) => entry.id === seasonId)

    if (!season) {
      return false
    }

    await loadSeasonPage(season, 1, false)
    return true
  }

  /**
   * Loads the next available page for the currently selected season.
   * Does nothing if there are no more episodes, no season link, or loading is already in progress.
   */
  async function handleLoadMoreSeasonEpisodes(): Promise<void> {
    const season = selectedSeason.value

    if (!season?.link || !hasMoreSeasonEpisodes.value || isSeasonLoadingMore.value) {
      return
    }

    await loadSeasonPage(season, currentSeasonPage.value + 1, true)
  }

  /**
   * Loads the detailed entry from the backend and ignores stale responses when props change quickly.
   * Also auto-selects the preferred season if available, or the first season with content.
   */
  async function loadEntryDetails(): Promise<void> {
    const requestId = ++latestRequestId

    isLoading.value = true
    errorMessage.value = null
    details.value = null
    resetSeasonState()

    try {
      const nextDetails = await getEntryDetails(
        options.source.value,
        options.entry.value,
        options.webUrl.value,
      )

      if (requestId !== latestRequestId) {
        return
      }

      details.value = nextDetails

      // Auto-select the stored season when available, otherwise keep the existing first-season fallback.
      if (nextDetails.seasons.length > 0 && !selectedSeasonId.value) {
        const restoredSeasonLoaded = await selectSeasonById(options.preferredSeasonId.value)

        if (!restoredSeasonLoaded) {
          const firstSeason = nextDetails.seasons[0]
          if (firstSeason && (firstSeason.episodes.length > 0 || firstSeason.link)) {
            await loadSeasonPage(firstSeason, 1, false)
          }
        }
      }
    } catch (error) {
      if (requestId !== latestRequestId) {
        return
      }

      details.value = null
      errorMessage.value =
        error instanceof Error ? error.message : t('errors.entry')
    } finally {
      if (requestId === latestRequestId) {
        isLoading.value = false
      }
    }
  }

  watch(
    () => [options.source.value, options.entry.value, options.webUrl.value],
    () => {
      void loadEntryDetails()
    },
    { immediate: true },
  )

  return {
    details,
    isLoading,
    errorMessage,
    seasonEpisodes,
    selectedSeasonId,
    selectedSeason,
    isSeasonLoading,
    isSeasonLoadingMore,
    seasonErrorMessage,
    hasMoreSeasonEpisodes,
    handleSeasonSelect,
    selectSeasonById,
    handleLoadMoreSeasonEpisodes,
  }
}
