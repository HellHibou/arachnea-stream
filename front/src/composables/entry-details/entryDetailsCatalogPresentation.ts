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
  const hasSeasons = computed(() => Boolean(options.details.value?.seasons.length))

  const seasonItems = computed<MediaItem[]>(() =>
    (options.details.value?.seasons ?? []).map((season) =>
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

  const entryEpisodeLabel = computed(() => {
    const firstSeasonName = options.details.value?.episodes
      .find((episode) => episode.seasonName)
      ?.seasonName
      ?.trim()

    return firstSeasonName || MSG_EPISODES
  })

  return {
    seasonItems,
    selectedSeasonLabel,
    displayedEpisodeItems,
    displayedEpisodeLabel: computed(() =>
      hasSeasons.value ? selectedSeasonLabel.value : entryEpisodeLabel.value,
    ),
    showSeasonPrompt: computed(() =>
      hasSeasons.value &&
      !options.selectedSeasonId.value &&
      !options.isSeasonLoading.value &&
      !options.seasonErrorMessage.value,
    ),
    showEmptySeasonState: computed(() =>
      hasSeasons.value &&
      Boolean(options.selectedSeasonId.value) &&
      !options.isSeasonLoading.value &&
      !options.seasonErrorMessage.value &&
      displayedEpisodeItems.value.length === 0,
    ),
    showEpisodeSection: computed(() => hasSeasons.value || displayedEpisodeItems.value.length > 0),
    showLoadMoreSeasonEpisodes: computed(() =>
      hasSeasons.value &&
      Boolean(options.selectedSeasonId.value) &&
      displayedEpisodeItems.value.length > 0 &&
      !options.seasonErrorMessage.value &&
      (options.hasMoreSeasonEpisodes.value || options.isSeasonLoadingMore.value),
    ),
  }
}
