import { computed, nextTick, shallowRef, watch, type Ref } from 'vue'

import type { EntryDetails, EntryEpisode } from '@/types/entry'
import type { MediaItem } from '@/types/media'

/**
 * Options accepted by the entry episode selection controller.
 */
interface UseEntryEpisodeSelectionOptions {
  /**
   * Loaded entry details driving the episode lists.
   */
  details: Ref<EntryDetails | null>
  /**
   * Episodes loaded from the currently selected season.
   */
  seasonEpisodes: Ref<EntryEpisode[]>
}

/**
 * Owns episode selection and next/previous navigation for one entry details view.
 *
 * @param options Reactive entry data sources used to derive the active episode list.
 * @returns Episode state and explicit actions used by the details player controller.
 */
export function entryEpisodeSelection(options: UseEntryEpisodeSelectionOptions) {
  /** The currently selected episode identifier. */
  const selectedEpisodeId = shallowRef<string | null>(null)
  /** Cache of all episodes encountered across seasons, keyed by episode id. */
  const allEpisodes = shallowRef<Map<string, EntryEpisode>>(new Map())
  /** Whether the first playable episode has been auto-selected for the current entry. */
  let initialEpisodeSelected = false

  /**
   * Indicates whether the current entry exposes seasons.
   *
   * @returns True if the entry has seasons.
   */
  const hasSeasons = computed(() => Boolean(options.details.value?.seasons.length))

  /**
   * Exposes the episodes currently rendered in the details view.
   *
   * Episodes always come from the season episodes, never from a top-level field.
   *
   * @returns Array of episodes from the currently selected season.
   */
  const displayedEpisodes = computed<EntryEpisode[]>(() => options.seasonEpisodes.value)

  /**
   * Tracks whether the first playable episode auto-selection has been applied
   * for the current entry load.
   */

  watch(displayedEpisodes, (eps) => {
    const map = allEpisodes.value
    for (const ep of eps) {
      map.set(ep.id, ep)
    }

    // Auto-select the first playable episode when episodes come directly
    // from get_entry (no seasons) and none is currently selected.
    if (!initialEpisodeSelected && !selectedEpisodeId.value && !hasSeasons.value) {
      const firstPlayable = eps.find((ep) => ep.players.length > 0)
      if (firstPlayable) {
        selectedEpisodeId.value = firstPlayable.id
        initialEpisodeSelected = true
      }
    }
  }, { immediate: true })

  /**
   * Exposes the episode currently selected in the built-in player.
   * Resolves from the accumulated episode cache to persist across season changes.
   *
   * @returns The selected episode, or null if none is selected.
   */
  const selectedEpisode = computed<EntryEpisode | null>(() => {
    if (!selectedEpisodeId.value) {
      return null
    }
    return allEpisodes.value.get(selectedEpisodeId.value) ?? null
  })

  /**
   * Exposes the playable episodes available for previous/next navigation.
   *
   * @returns Array of episodes that have at least one player.
   */
  const navigableEpisodes = computed<EntryEpisode[]>(() =>
    displayedEpisodes.value.filter((episode) => episode.players.length > 0),
  )

  /**
   * Exposes the index of the currently selected playable episode.
   *
   * @returns The index of the selected episode in navigableEpisodes, or -1 if not found.
   */
  const selectedNavigableEpisodeIndex = computed(() =>
    navigableEpisodes.value.findIndex((episode) => episode.id === selectedEpisodeId.value),
  )

  /**
   * Indicates whether the previous playable episode can be selected.
   *
   * @returns True if there is a previous playable episode.
   */
  const hasPreviousEpisode = computed(() => selectedNavigableEpisodeIndex.value > 0)

  /**
   * Indicates whether the next playable episode can be selected.
   *
   * @returns True if there is a next playable episode.
   */
  const hasNextEpisode = computed(() =>
    selectedNavigableEpisodeIndex.value >= 0 &&
    selectedNavigableEpisodeIndex.value < navigableEpisodes.value.length - 1,
  )

  /**
   * Clears the currently selected episode and the accumulated episode cache.
   */
  function resetSelectedEpisode(): void {
    selectedEpisodeId.value = null
    allEpisodes.value.clear()
  }

  /**
   * Updates the built-in player episode selection to match the provided episode.
   *
   * @param episode - Episode that should become active.
   */
  function selectEpisode(episode: EntryEpisode): void {
    selectedEpisodeId.value = episode.id
  }

  /**
   * Scrolls back to the player title once the DOM has applied the latest episode selection.
   */
  function scrollToTitleSection(): void {
    void nextTick(() => {
      const element = document.getElementById('title-section')
      if (!element) {
        return
      }

      window.scrollTo({ top: element.offsetTop, behavior: 'smooth' })
    })
  }

  /**
   * Restores the selected episode from a stored identifier when it is available in the current list.
   *
   * @param episodeId Preferred episode identifier persisted for the current entry.
   * @returns `true` when the episode could be restored.
   */
  function selectEpisodeById(episodeId: string | null): boolean {
    if (!episodeId) {
      return false
    }

    const episode = displayedEpisodes.value.find((entry) => entry.id === episodeId)

    if (!episode || episode.players.length === 0) {
      return false
    }

    selectEpisode(episode)
    return true
  }

  /**
   * Switches the built-in player to the selected episode.
   *
   * @param item Episode card selected from the episode collection.
   */
  function handleEpisodeSelect(item: MediaItem) {
    const episode = displayedEpisodes.value.find((entry) => entry.id === item.id)

    if (!episode || episode.players.length === 0) {
      return
    }

    selectEpisode(episode)
    scrollToTitleSection()
  }

  /**
   * Moves the built-in player to the previous or next playable episode.
   *
   * @param offset - Relative episode offset applied to the current episode selection (-1 for previous, 1 for next).
   */
   function handleEpisodeStep(offset: -1 | 1): void {
     const nextEpisode =
       navigableEpisodes.value[selectedNavigableEpisodeIndex.value + offset] ?? null

     if (!nextEpisode) {
       return
     }

     selectEpisode(nextEpisode)
   }

   return {
     selectedEpisodeId,
     displayedEpisodes,
     selectedEpisode,
     hasPreviousEpisode,
     hasNextEpisode,
     resetSelectedEpisode,
     selectEpisodeById,
     handleEpisodeSelect,
     handleEpisodeStep,
   }
 }
