import { computed, type Ref } from 'vue'

import type { EntryDetails, EntryEpisode, EntrySeason } from '@/types/entry'
import type { MediaItem } from '@/types/media'

import { toEntryEpisodeMediaItem, toEntrySeasonMediaItem } from './entryDetailsMediaItems'
import { MSG_EPISODES } from '@/i18n'

/**
 * Options accepted by the entry details catalog presentation mapper.
 */
interface UseEntryDetailsCatalogPresentationOptions {
  /**
   * Loaded detailed entry rendered by the view.
   */
  details: Ref<EntryDetails | null>
  /**
   * Backend source used to build interactive media items.
   */
  source: Ref<string>
  /**
   * Episodes currently displayed in the details view.
   */
  displayedEpisodes: Ref<EntryEpisode[]>
  /**
   * Season currently selected in the details view.
   */
  selectedSeason: Ref<EntrySeason | null>
  /**
   * Identifier of the selected season.
   */
  selectedSeasonId: Ref<string | null>
  /**
   * Current season loading error message.
   */
  seasonErrorMessage: Ref<string | null>
  /**
   * Indicates whether the selected season is being loaded.
   */
  isSeasonLoading: Ref<boolean>
  /**
   * Indicates whether the next season page is currently loading.
   */
  isSeasonLoadingMore: Ref<boolean>
  /**
   * Indicates whether another season page can still be requested.
   */
  hasMoreSeasonEpisodes: Ref<boolean>
}

/**
 * Provides the card collections and section states rendered below the hero area.
 *
 * @param options Reactive data sources already normalized by the feature composables.
 * @returns Pure computed values consumed by the episodes subcomponent.
 */
export function entryDetailsCatalogPresentation(
  options: UseEntryDetailsCatalogPresentationOptions,
) {
  /** Reactive array of seasons from the entry details. */
  const seasons = computed(() => options.details.value?.seasons ?? [])
  /** Whether the entry has any seasons. */
  const hasSeasons = computed(() => seasons.value.length > 0)

  /**
   * Whether there is exactly one season with no label and at least one episode or link.
   *
   * @returns True for single implicit seasons that should auto-show episodes.
   */
  const hasSingleImplicitSeason = computed(() => {
    const firstSeason = seasons.value[0]
    return seasons.value.length === 1 &&
      firstSeason &&
      !firstSeason.label &&
      (firstSeason.episodes.length > 0 || Boolean(firstSeason.link))
  })

  /**
   * Whether season selection UI should be shown.
   *
   * @returns True when there are multiple seasons or a single labeled season.
   */
  const showSeasonSelection = computed(() => {
    const firstSeason = seasons.value[0]
    return seasons.value.length > 1 || (seasons.value.length === 1 && Boolean(firstSeason?.label))
  })

  /**
   * Season items mapped to MediaItem format for display.
   *
   * @returns Array of season media items with poster and landscape images.
   */
  const seasonItems = computed<MediaItem[]>(() =>
    seasons.value.map((season) =>
      toEntrySeasonMediaItem(
        season,
        options.details.value?.imagePosterUrl ?? null,
        options.details.value?.imageLandscapeUrl ?? null,
        options.source.value,
      ),
    ),
  )

  /**
   * Displayed episodes mapped to MediaItem format.
   *
   * @returns Array of episode media items.
   */
  const displayedEpisodeItems = computed<MediaItem[]>(() =>
    options.displayedEpisodes.value.map((episode) =>
      toEntryEpisodeMediaItem(episode, options.source.value),
    ),
  )

  /** The label of the currently selected season, or default episodes message. */
  const selectedSeasonLabel = computed(() => options.selectedSeason.value?.label ?? MSG_EPISODES)

  /**
   * The label to display for the episode section.
   *
   * @returns Season label if season selection is shown, otherwise default episodes message.
   */
  const displayedEpisodeLabel = computed(() =>
    showSeasonSelection.value ? selectedSeasonLabel.value : MSG_EPISODES,
  )

  /**
   * Whether to show the season selection prompt.
   *
   * @returns True when season selection is available but no season is selected and not loading.
   */
  const showSeasonPrompt = computed(() =>
    showSeasonSelection.value &&
    !options.selectedSeasonId.value &&
    !options.isSeasonLoading.value &&
    !options.seasonErrorMessage.value,
  )

  /**
   * Whether to show the empty season state message.
   *
   * @returns True when a season is selected but has no episodes and is not loading.
   */
  const showEmptySeasonState = computed(() =>
    showSeasonSelection.value &&
    Boolean(options.selectedSeasonId.value) &&
    !options.isSeasonLoading.value &&
    !options.seasonErrorMessage.value &&
    displayedEpisodeItems.value.length === 0,
  )

  /**
   * Whether to show the episode section at all.
   *
   * @returns True when there are seasons, a single implicit season, or displayed episodes.
   */
  const showEpisodeSection = computed(() =>
    showSeasonSelection.value || hasSingleImplicitSeason.value || displayedEpisodeItems.value.length > 0,
  )

  /**
   * Whether to show the "load more" button for season episodes.
   *
   * @returns True when there are episodes, no error, and more episodes can be loaded.
   */
  const showLoadMoreSeasonEpisodes = computed(() =>
    Boolean(options.selectedSeasonId.value) &&
    displayedEpisodeItems.value.length > 0 &&
    !options.seasonErrorMessage.value &&
    (options.hasMoreSeasonEpisodes.value || options.isSeasonLoadingMore.value),
  )

  return {
    seasonItems,
    selectedSeasonLabel,
    displayedEpisodeItems,
    displayedEpisodeLabel,
    showSeasonPrompt,
    showEmptySeasonState,
    showEpisodeSection,
    showLoadMoreSeasonEpisodes,
    hasSingleImplicitSeason,
    showSeasonSelection,
  }
}
