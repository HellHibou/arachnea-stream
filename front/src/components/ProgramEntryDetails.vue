<script setup lang="ts">
import { computed, shallowRef, toRef, watch } from 'vue'

import type { MediaItem, ThumbnailImageFit } from '@/types/media'
import type { EntryEpisode } from '@/types/entry'

import EntryDetails from './EntryDetails.vue'
import EntryDetailsCatalogSection from './entry-details/EntryDetailsCatalogSection.vue'
import { useI18n } from '@/i18n'
import { entryDetailsData } from '@/composables/entry-details/entryDetailsData'
import { entryEpisodeSelection } from '@/composables/entry-details/entryEpisodeSelection'
import { entryDetailsPresentation } from '@/composables/entry-details/entryDetailsPresentation'
import { entryVideoPlayer } from '@/composables/entry-details/entryVideoPlayer'
import { getSeasonEpisodes } from '@/services/rustify'
import { type EntryBookmark, useStorage } from '@/services/storage'

/**
  * Props accepted by the featured entry details component.
  */
interface Props {
  /**
   * Backend source used to resolve the entry.
   */
  source: string
  /**
   * Absolute entry URL passed to the backend `get_entry` function.
   */
  entry: string
  /**
   * Public web URL opened by the primary action.
   */
  webUrl: string | null
  /**
   * Indicates whether the trailer should be used as the page background when available.
   * @default true
   */
  useTrailerAsBackground?: boolean
  /**
   * Indicates whether the full-page background should animate.
   * @default false
   */
  isBackgroundAnimated?: boolean
  /**
   * Controls whether the background image should be fully visible or cropped.
   * @default 'contain'
   */
  backgroundImageFit?: ThumbnailImageFit
  /**
   * Indicates whether to use catalog banners as background.
   * @default true
   */
  useCatalogBannersAsBackground?: boolean
}

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
  useTrailerAsBackground: true,
  isBackgroundAnimated: false,
  backgroundImageFit: 'contain',
  useCatalogBannersAsBackground: true,
})
/** Internationalization utilities. */
const { t } = useI18n()

/** Reactive reference to the source prop. */
const source = toRef(props, 'source')
/** Reactive reference to the entry prop. */
const entry = toRef(props, 'entry')
/** Reactive reference to the webUrl prop. */
const webUrl = toRef(props, 'webUrl')
/** Storage service instance. */
const storage = useStorage()
/** Application parameters from persistent storage. */
const parameters = storage.getParameters()
/** Latest playback time from the media player. */
const latestPlaybackTime = shallowRef<number | null>(null)
/** Key to track restored episode bookmark state. */
const restoredEpisodeBookmarkKey = shallowRef<string | null>(null)
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

/** Entry details data composable results. */
const {
  /** Entry details data. */
  details,
  /** Whether the entry details are currently loading. */
  isLoading,
  /** Error message from entry details loading. */
  errorMessage,
  /** Episodes for the selected season. */
  seasonEpisodes,
  /** Currently selected season ID. */
  selectedSeasonId,
  /** Currently selected season data. */
  selectedSeason,
  /** Whether the season is currently loading. */
  isSeasonLoading,
  /** Whether more season episodes are currently loading. */
  isSeasonLoadingMore,
  /** Error message from season loading. */
  seasonErrorMessage,
  /** Whether there are more season episodes available to load. */
  hasMoreSeasonEpisodes,
  /** Function to handle season selection from data. */
  handleSeasonSelect: handleDataSeasonSelect,
  /** Function to select a season by ID. */
  selectSeasonById,
  /** Function to load more season episodes. */
  handleLoadMoreSeasonEpisodes,
} = entryDetailsData({
  source,
  entry,
  webUrl,
  preferredSeasonId,
})

/** Entry episode selection composable results. */
const {
  /** Currently selected episode ID. */
  selectedEpisodeId,
  /** Displayed episodes for the current selection. */
  displayedEpisodes,
  /** Currently selected episode data. */
  selectedEpisode,
  /** Whether there is a previous episode available. */
  hasPreviousEpisode,
  /** Whether there is a next episode available in the current season. */
  hasNextEpisode: hasNextEpisodeInSeason,
  /** Function to reset the selected episode. */
  resetSelectedEpisode,
  /** Function to select an episode by ID. */
  selectEpisodeById,
  /** Function to handle episode selection. */
  handleEpisodeSelect,
  /** Function to handle episode step navigation. */
  handleEpisodeStep,
} = entryEpisodeSelection({
  details,
  seasonEpisodes,
})

/** Entry video player composable results. */
const {
  /** Currently active language key. */
  activeLanguageKey,
  /** Available language options. */
  availableLanguages,
  /** Filtered list of available players. */
  filteredPlayers,
  /** Currently active player ID. */
  activePlayerId,
  /** Current media source for the player. */
  mediaSource,
  /** URL to open the media in a new tab. */
  mediaOpenUrl,
  /** Whether the media player is currently loading. */
  isMediaPlayerLoading,
  /** Error message from the media player. */
  mediaPlayerErrorMessage,
  /** Poster image returned by the selected player resolver. */
  resolvedPlayerPosterUrl,
  /** URL of the trailer media. */
  trailerUrl,
  /** Trailer media source for the player. */
  trailerMediaSource,
  /** Whether to show the trailer player. */
  showTrailerPlayer,
  /** Currently selected playable title. */
  selectedPlayableTitle,
  /** Whether to show the media player. */
  showMediaPlayer,
  /** Whether to show the trailer action button. */
  showTrailerAction,
  /** Label for the trailer action button. */
  trailerActionLabel,
  /** Whether to show the language selector. */
  showLanguageSelector,
  /** Whether to show the player selector. */
  showPlayerSelector,
  /** Whether to show player controls. */
  showPlayerControls,
  /** Function to activate the media player. */
  activateMediaPlayer,
  /** Function to handle trailer toggle. */
  handleTrailerToggle,
  /** Function to remember current language selection. */
  rememberCurrentLanguage,
  /** Function to remember current player selection. */
  rememberCurrentPlayer,
  handleMediaSourceError,
} = entryVideoPlayer({
  details,
  selectedPlayableItem: selectedEpisode,
})

/** Entry details presentation composable results. */
const {
  /** Display title for the entry. */
  displayTitle,
  /** Display description for the entry. */
  displayDescription,
  /** Alternative title label for the entry. */
  alternativeTitleLabel,
  /** URL of the poster frame image. */
  posterFrameImageUrl,
  /** Whether the poster frame should use contain sizing. */
  posterFrameUsesContain,
  /** URL of the hero background image. */
  heroBackgroundUrl,
  /** URL of the portrait-oriented hero background image. */
  heroBackgroundPortraitUrl,
  /** URL of the landscape-oriented hero background image. */
  heroBackgroundLandscapeUrl,
  /** Display label for the release date. */
  displayReleaseDateLabel,
  /** Display label for the expiration date. */
  displayExpireLabel,
  /** Display label for the duration. */
  displayDurationLabel,
  /** List of metadata badges to display. */
  metadataBadges,
  /** Genre text to display. */
  genreText,
  /** List of topic chips to display. */
  topicChips,
  /** Casting text to display. */
  castingText,
  /** Director text to display. */
  directorText,
  /** Season items for the catalog section. */
  seasonItems,
  /** Label for the selected season. */
  selectedSeasonLabel,
  /** Displayed episode items for the catalog section. */
  displayedEpisodeItems,
  /** Label for the displayed episodes. */
  displayedEpisodeLabel,
  /** Whether to show the season prompt. */
  showSeasonPrompt,
  /** Whether to show the empty season state. */
  showEmptySeasonState,
  /** Whether to show the episode section. */
  showEpisodeSection,
  /** Whether to show the load more season episodes button. */
  showLoadMoreSeasonEpisodes,
} = entryDetailsPresentation({
  details,
  source,
  selectedEpisode,
  displayedEpisodes,
  selectedSeason,
  selectedSeasonId,
  seasonErrorMessage,
  isSeasonLoading,
  isSeasonLoadingMore,
  hasMoreSeasonEpisodes,
})

/** Resets entry state when source, entry, or webUrl props change. */
watch([source, entry, webUrl], () => {
  latestPlaybackTime.value = null
  restoredEpisodeBookmarkKey.value = null
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
 * Resolves the trailer URL used by the full-page background when the option is enabled.
 */
const backgroundVideoUrl = computed(() =>
  props.useTrailerAsBackground
    ? trailerUrl.value
    : null,
)

/**
 * Exposes the playback position that should be restored for the active episode video.
 */
const initialPlaybackTime = computed(() => {
  const bookmark = currentEntryBookmark.value
  const currentEpisodeId = selectedEpisode.value?.id ?? null

  if (!bookmark || bookmark.selectedItemId !== currentEpisodeId) {
    return null
  }

  return bookmark.playbackTime
})

/**
 * Exposes the selected player title image, falling back to episode or entry imagery.
 */
const mediaPosterUrl = computed(() =>
  resolvedPlayerPosterUrl.value ??
  selectedEpisode.value?.previewUrl ??
  details.value?.imageLandscapeUrl ??
  details.value?.imagePosterUrl ??
  null,
)

/**
 * Exposes the entry poster used as the initial poster for trailers in the integrated Video.js player.
 */
const trailerPosterUrl = computed(() => details.value?.imagePosterUrl ?? null)

/**
 * Exposes the logo overlaid on top of the media poster in the integrated Video.js player.
 */
const mediaOverlayLogoUrl = computed(() =>
  details.value?.logoUrl ?? null,
)

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
 * Stores one bookmark payload for the current entry.
 *
 * @param bookmark Bookmark payload to persist.
 */
function saveEntryBookmark(bookmark: EntryBookmark) {
  storage.setEntryBookmark(source.value, entry.value, bookmark)
}

/**
 * Stores the bookmark using the current selection state.
 *
 * @param playbackTime Playback position in seconds to persist for the active entry.
 */
function saveCurrentBookmark(playbackTime: number | null = latestPlaybackTime.value) {
  saveEntryBookmark({
    groupId: selectedSeasonId.value,
    selectedItemId: selectedEpisodeId.value,
    playbackTime,
  })
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
  return episodes.find((episode) => episode.players.length > 0) ?? null
}

/**
 * Applies the side effects shared by manual and automatic episode navigation.
 *
 * @param autoplay Indicates whether the next selected episode should start automatically.
 */
function applyEpisodeSelectionEffects(autoplay: boolean) {
  beginEpisodeSelectionTransition(autoplay)
  activateMediaPlayer()
  latestPlaybackTime.value = null

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
 * Loads a new season without affecting the current player state.
 *
 * @param item Season card selected from the season collection.
 */
function handleSeasonSelect(item: MediaItem) {
  cancelPendingEpisodeNavigation()
  latestPlaybackTime.value = null
  void handleDataSeasonSelect(item)

  if (!isBookmarked.value) {
    return
  }

  saveEntryBookmark({
    groupId: item.id,
    selectedItemId: null,
    playbackTime: null,
  })
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
 * Stores the latest playback position emitted by the integrated Video.js renderer.
 *
 * @param playbackTime Playback position in seconds, or `null` when it should be cleared.
 */
function handlePlaybackProgressUpdate(playbackTime: number | null) {
  latestPlaybackTime.value = playbackTime

  if (!isBookmarked.value) {
    return
  }

  saveCurrentBookmark(playbackTime)
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
 * Starts the next playable episode automatically when the preference is enabled.
 */
async function handleMediaPlaybackEnded() {
  if (!parameters.isEpisodeAutoplayEnabled.value) {
    return
  }

  await selectNextPlayableEpisode(true)
}

</script>

<template>
  <EntryDetails
    :error-message="errorMessage"
    :is-loading="isLoading"
    :has-content="Boolean(details)"
    :background-video-url="backgroundVideoUrl"
    :hero-background-url="heroBackgroundUrl"
    :hero-background-portrait-url="heroBackgroundPortraitUrl"
    :hero-background-landscape-url="heroBackgroundLandscapeUrl"
    :is-background-animated="props.isBackgroundAnimated"
    :background-image-fit="props.backgroundImageFit"
    :use-catalog-banners-as-background="props.useCatalogBannersAsBackground"
    :display-title="displayTitle"
    :poster-frame-image-url="posterFrameImageUrl"
    :poster-frame-uses-contain="posterFrameUsesContain"
    :show-trailer-action="showTrailerAction"
    :trailer-action-label="trailerActionLabel"
    :entry-url="details?.entryUrl ?? null"
    :source="source"
    :alternative-title-label="alternativeTitleLabel"
    :selected-playable-title="selectedPlayableTitle"
    show-adjacent-navigation
    :has-previous-playable="hasPreviousEpisode"
    :has-next-playable="hasNextEpisode"
    :is-bookmarked="isBookmarked"
    :show-trailer-player="showTrailerPlayer"
    :show-media-player="showMediaPlayer"
    :trailer-media-source="trailerMediaSource"
    :media-source="mediaSource"
    :media-open-url="mediaOpenUrl"
    :is-media-player-loading="isMediaPlayerLoading"
    :media-player-error-message="mediaPlayerErrorMessage"
    :show-player-controls="showPlayerControls"
    :show-language-selector="showLanguageSelector"
    :show-player-selector="showPlayerSelector"
    :available-languages="availableLanguages"
    :active-language-key="activeLanguageKey"
    :filtered-players="filteredPlayers"
    :active-player-id="activePlayerId"
    :media-poster-url="mediaPosterUrl"
    :trailer-poster-url="trailerPosterUrl"
    :media-overlay-logo-url="mediaOverlayLogoUrl"
    :display-description="displayDescription"
    :initial-playback-time="initialPlaybackTime"
    :media-autoplay="pendingEpisodeAutoplay"
    :prefer-persisted-media-surface="shouldKeepMediaSurfaceMounted"
    show-autoplay-toggle
    :is-autoplay-enabled="parameters.isEpisodeAutoplayEnabled.value"
    :metadata-badges="metadataBadges"
    :topic-chips="topicChips"
    :genre-text="genreText"
    :year-label="details?.yearLabel ?? null"
    :display-release-date-label="displayReleaseDateLabel"
    :display-expire-label="displayExpireLabel"
     :content-advisor-label="details?.contentAdvisorLabel ?? null"
     :audio-language-label="details?.audioLanguageLabel ?? null"
     :subtitle-language-label="details?.subtitleLanguageLabel ?? null"
     :display-duration-label="displayDurationLabel"
     :casting-text="castingText"
     :director-text="directorText"
     :score="details?.score ?? null"
    @toggle-trailer="handlePlayerTrailerToggle"
    @step-playable="handlePlayerEpisodeStep"
    @step-playable-autoplay="handlePlayerControlStep"
    @toggle-bookmark="handleBookmarkToggle"
    @update:active-language-key="handleActiveLanguageKeyUpdate"
    @remember-current-language="rememberCurrentLanguage"
    @update:active-player-id="handleActivePlayerIdUpdate"
    @remember-current-player="rememberCurrentPlayer"
    @update:playback-progress="handlePlaybackProgressUpdate"
    @update:is-autoplay-enabled="handleEpisodeAutoplayPreferenceUpdate"
    @playback-started="handleMediaPlaybackStarted"
    @playback-ended="handleMediaPlaybackEnded"
    @source-error="handleMediaSourceError"
  >
    <EntryDetailsCatalogSection
      :group-items="seasonItems"
      :selected-group-label="selectedSeasonLabel"
      :show-item-section="showEpisodeSection"
      :group-error-message="seasonErrorMessage"
      :is-group-loading="isSeasonLoading"
      :show-group-prompt="showSeasonPrompt"
      :show-empty-group-state="showEmptySeasonState"
      :displayed-items="displayedEpisodeItems"
      :displayed-item-label="displayedEpisodeLabel"
      :show-load-more-items="showLoadMoreSeasonEpisodes"
      :is-loading-more-items="isSeasonLoadingMore"
      :group-section-label="t('entry.season')"
      :item-section-label="t('entry.episodes')"
      :select-field-label="t('entry.season')"
      :state-eyebrow="t('entry.season')"
      :loading-state-message="t('entry.loadingSeasonMessage', { season: selectedSeasonLabel })"
      :prompt-title="t('entry.seasonPrompt')"
      :prompt-message="t('entry.seasonPromptMessage')"
      :empty-state-title="t('entry.emptyEpisodeTitle')"
      :empty-state-message="t('entry.emptyEpisodeMessage', { season: selectedSeasonLabel })"
      :show-service-logo="false"
      @select-group="handleSeasonSelect"
      @select-item="handlePlayerEpisodeSelect"
      @load-more="handleLoadMoreSeasonEpisodes"
    />
  </EntryDetails>
</template>
