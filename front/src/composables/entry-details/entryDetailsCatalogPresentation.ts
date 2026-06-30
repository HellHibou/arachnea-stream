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
  const seasons = computed(() => options.details.value?.seasons ?? [])
  const hasSeasons = computed(() => seasons.value.length > 0)

  const hasSingleImplicitSeason = computed(() => {
    const firstSeason = seasons.value[0]
    return seasons.value.length === 1 &&
      firstSeason &&
      !firstSeason.label &&
      (firstSeason.episodes.length > 0 || Boolean(firstSeason.link))
  })

  const showSeasonSelection = computed(() => {
    const firstSeason = seasons.value[0]
    return seasons.value.length > 1 || (seasons.value.length === 1 && Boolean(firstSeason?.label))
  })

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

  const displayedEpisodeItems = computed<MediaItem[]>(() =>
    options.displayedEpisodes.value.map((episode) =>
      toEntryEpisodeMediaItem(episode, options.source.value),
    ),
  )

  const selectedSeasonLabel = computed(() => options.selectedSeason.value?.label ?? MSG_EPISODES)

  const displayedEpisodeLabel = computed(() =>
    showSeasonSelection.value ? selectedSeasonLabel.value : MSG_EPISODES,
  )

  const showSeasonPrompt = computed(() =>
    showSeasonSelection.value &&
    !options.selectedSeasonId.value &&
    !options.isSeasonLoading.value &&
    !options.seasonErrorMessage.value,
  )

  const showEmptySeasonState = computed(() =>
    showSeasonSelection.value &&
    Boolean(options.selectedSeasonId.value) &&
    !options.isSeasonLoading.value &&
    !options.seasonErrorMessage.value &&
    displayedEpisodeItems.value.length === 0,
  )

  const showEpisodeSection = computed(() =>
    showSeasonSelection.value || hasSingleImplicitSeason.value || displayedEpisodeItems.value.length > 0,
  )

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
