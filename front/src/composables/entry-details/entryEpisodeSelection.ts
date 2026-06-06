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
  const selectedEpisodeId = shallowRef<string | null>(null)
  const allEpisodes = shallowRef<Map<string, EntryEpisode>>(new Map())

  /**
   * Indicates whether the current entry exposes seasons.
   */
  const hasSeasons = computed(() => Boolean(options.details.value?.seasons.length))

  /**
   * Exposes the episodes currently rendered in the details view.
   */
  const displayedEpisodes = computed<EntryEpisode[]>(() =>
    hasSeasons.value ? options.seasonEpisodes.value : (options.details.value?.episodes ?? []),
  )

  /**
   * Accumulates episodes from all loaded seasons into a persistent map.
   * This lets the player keep the same episode selected when switching seasons.
   * Overwrites existing entries to keep data fresh on entry reloads.
   */
  /**
   * Tracks whether the first playable episode auto-selection has been applied
   * for the current non-seasonal entry load.
   */
  let initialEpisodeSelected = false

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
   */
  const selectedEpisode = computed<EntryEpisode | null>(() => {
    if (!selectedEpisodeId.value) {
      return null
    }
    return allEpisodes.value.get(selectedEpisodeId.value) ?? null
  })

  /**
   * Exposes the playable episodes available for previous/next navigation.
   */
  const navigableEpisodes = computed<EntryEpisode[]>(() =>
    displayedEpisodes.value.filter((episode) => episode.players.length > 0),
  )

  /**
   * Exposes the index of the currently selected playable episode.
   */
  const selectedNavigableEpisodeIndex = computed(() =>
    navigableEpisodes.value.findIndex((episode) => episode.id === selectedEpisodeId.value),
  )

  /**
   * Indicates whether the previous playable episode can be selected.
   */
  const hasPreviousEpisode = computed(() => selectedNavigableEpisodeIndex.value > 0)

  /**
   * Indicates whether the next playable episode can be selected.
   */
  const hasNextEpisode = computed(() =>
    selectedNavigableEpisodeIndex.value >= 0 &&
    selectedNavigableEpisodeIndex.value < navigableEpisodes.value.length - 1,
  )

  /**
   * Clears the currently selected episode and the accumulated episode cache.
   */
  function resetSelectedEpisode() {
    selectedEpisodeId.value = null
    allEpisodes.value.clear()
  }

  /**
   * Updates the built-in player episode selection to match the provided episode.
   *
   * @param episode Episode that should become active.
   */
  function selectEpisode(episode: EntryEpisode) {
    selectedEpisodeId.value = episode.id
  }

  /**
   * Scrolls back to the player title once the DOM has applied the latest episode selection.
   */
  function scrollToTitleSection() {
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
   * @param offset Relative episode offset applied to the current episode selection.
   */
   function handleEpisodeStep(offset: -1 | 1) {
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
