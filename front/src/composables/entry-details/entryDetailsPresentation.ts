import { type Ref } from 'vue'

import type { EntryDetails, EntryEpisode, EntrySeason } from '@/types/entry'
import { entryDetailsCatalogPresentation } from './entryDetailsCatalogPresentation'
import { entryDetailsHeroPresentation } from './entryDetailsHeroPresentation'
import { entryDetailsMetadataPresentation } from './entryDetailsMetadataPresentation'

/**
 * Options accepted by the entry details presentation mapper.
 */
interface UseEntryDetailsPresentationOptions {
  /**
   * Loaded detailed entry rendered by the view.
   */
  details: Ref<EntryDetails | null>
  /**
   * Backend source used to build interactive media items.
   */
  source: Ref<string>
  /**
   * Episode currently selected in the built-in player.
   */
  selectedEpisode: Ref<EntryEpisode | null>
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
 * Provides the presentation-only derived state needed by the entry details template.
 *
 * @param options Reactive data sources already normalized by the feature composables.
 * @returns Pure computed values consumed by the details subcomponents.
 */
export function entryDetailsPresentation(options: UseEntryDetailsPresentationOptions) {
  const heroPresentation = entryDetailsHeroPresentation({
    details: options.details,
    selectedEpisode: options.selectedEpisode,
  })

  const metadataPresentation = entryDetailsMetadataPresentation({
    details: options.details,
  })

  const catalogPresentation = entryDetailsCatalogPresentation({
    details: options.details,
    source: options.source,
    displayedEpisodes: options.displayedEpisodes,
    selectedSeason: options.selectedSeason,
    selectedSeasonId: options.selectedSeasonId,
    seasonErrorMessage: options.seasonErrorMessage,
    isSeasonLoading: options.isSeasonLoading,
    isSeasonLoadingMore: options.isSeasonLoadingMore,
    hasMoreSeasonEpisodes: options.hasMoreSeasonEpisodes,
  })

  return {
    ...heroPresentation,
    ...metadataPresentation,
    ...catalogPresentation,
  }
}
