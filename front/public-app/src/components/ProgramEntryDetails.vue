<script setup lang="ts">
import { toRef, useTemplateRef } from 'vue'
import type { ThumbnailImageFit } from '@/types/media'
import { useI18n } from '@/i18n'
import { useStorage } from '@/services/storage'
import EntryDetails from './EntryDetails.vue'
import EntryDetailsCatalogSection from './entry-details/EntryDetailsCatalogSection.vue'
import MediaCardCollection from './MediaCardCollection.vue'
import { entryDetailsData } from '@/composables/entry-details/entryDetailsData'
import { entryEpisodeSelection } from '@/composables/entry-details/entryEpisodeSelection'
import { entryVideoPlayer } from '@/composables/entry-details/entryVideoPlayer'
import { entryDetailsPresentation } from '@/composables/entry-details/entryDetailsPresentation'
import { useEntryBookmark } from '@/composables/entry-details/useEntryBookmark'
import { useEntryEpisodeNavigation } from '@/composables/entry-details/useEntryEpisodeNavigation'
import { useEntryMediaSurface } from '@/composables/entry-details/useEntryMediaSurface'
import { useEntryRecommendationsRail } from '@/composables/entry-details/useEntryRecommendationsRail'

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
  /**
   * Mode used to render embedded iframe players.
   * @default 'safe'
   */
  securityMode?: 'unsafe' | 'confirmation' | 'safe'
}

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
  useTrailerAsBackground: true,
  isBackgroundAnimated: false,
  backgroundImageFit: 'contain',
  useCatalogBannersAsBackground: true,
  securityMode: 'safe',
})
const { t } = useI18n()
const source = toRef(props, 'source')
const entry = toRef(props, 'entry')
const webUrl = toRef(props, 'webUrl')
const storage = useStorage()
const parameters = storage.getParameters()

/** Bookmark preferences are available before the data controller chooses its season. */
const bookmark = useEntryBookmark({ source, entry, storage })
const { isBookmarked, preferredSeasonId } = bookmark

const data = entryDetailsData({
  source,
  entry,
  webUrl,
  preferredSeasonId,
})
const {
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
  handleLoadMoreSeasonEpisodes,
} = data

const recommendationsRailRef = useTemplateRef<{ $el: HTMLElement }>('recommendationsRailRef')
const recommendations = useEntryRecommendationsRail({ source, details, rail: recommendationsRailRef })
const {
  recommendationItems, isRecommendationsLoading, isRecommendationsLoadingMore,
  recommendationsErrorMessage, hasMoreRecommendations, showRecommendations,
  handleLoadMoreRecommendations, handleRecommendationSelect,
} = recommendations

const selection = entryEpisodeSelection({
  details,
  seasonEpisodes,
})
const {
  displayedEpisodes,
  selectedEpisode,
  hasPreviousEpisode,
  previousNavigableEpisode,
  nextNavigableEpisode,
} = selection

const player = entryVideoPlayer({
  details,
  selectedPlayableItem: selectedEpisode,
  securityMode: toRef(props, 'securityMode'),
  useTrailerAsBackground: toRef(props, 'useTrailerAsBackground'),
})
const {
  activeLanguageKey,
  availableLanguages,
  filteredPlayers,
  activePlayerId,
  mediaSource,
  mediaOpenUrl,
  isMediaPlayerLoading,
  mediaPlayerErrorMessage,
  trailerMediaSource,
  showTrailerPlayer,
  selectedPlayableTitle,
  showMediaPlayer,
  showTrailerAction,
  trailerActionLabel,
  showLanguageSelector,
  showPlayerSelector,
  showPlayerControls,
  rememberCurrentLanguage,
  rememberCurrentPlayer,
  handleMediaSourceError,
} = player

const presentation = entryDetailsPresentation({
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
const {
  displayTitle,
  displayDescription,
  alternativeTitleLabel,
  posterFrameImageUrl,
  posterFrameUsesContain,
  heroBackgroundUrl,
  heroBackgroundPortraitUrl,
  heroBackgroundLandscapeUrl,
  displayReleaseDateLabel,
  displayExpireLabel,
  displayDurationLabel,
  metadataBadges,
  genreText,
  topicChips,
  castingText,
  directorText,
  seasonItems,
  selectedSeasonLabel,
  displayedEpisodeItems,
  displayedEpisodeLabel,
  showSeasonPrompt,
  showEmptySeasonState,
  showEpisodeSection,
  showLoadMoreSeasonEpisodes,
} = presentation

const navigation = useEntryEpisodeNavigation({
  source, entry, webUrl, data, selection, player, bookmark, parameters,
  // Called only by navigation/playback events after bookmark selection is bound below.
  saveCurrentBookmark: (time) => bookmarkSelection.saveCurrentBookmark(time),
})
const {
  pendingEpisodeAutoplay, shouldKeepMediaSurfaceMounted, hasNextEpisode,
  handleSeasonSelect, handlePlayerEpisodeSelect, handlePlayerEpisodeStep, handlePlayerControlStep,
  handlePlayerTrailerToggle, handleActiveLanguageKeyUpdate, handleActivePlayerIdUpdate,
  handleEpisodeAutoplayPreferenceUpdate, handleMediaPlaybackStarted, handleMediaPlaybackEnded,
} = navigation

/** Restore the selected episode after the entry and media transition reset watchers. */
const bookmarkSelection = bookmark.bindSelection({ data, selection, presentation, hasNextEpisode })
const { handleBookmarkToggle, handlePlaybackProgressUpdate } = bookmarkSelection

const {
  previousPlayableItem, nextPlayableItem, backgroundVideoSource, initialPlaybackTime,
  mediaPosterUrl, trailerPosterUrl, mediaOverlayLogoUrl,
} = useEntryMediaSurface({
  source, useTrailerAsBackground: toRef(props, 'useTrailerAsBackground'),
  data, selection, player, currentEntryBookmark: bookmark.currentEntryBookmark,
})
</script>

<template>
  <EntryDetails
    :error-message="errorMessage"
    :is-loading="isLoading"
    :has-content="Boolean(details)"
    :background-video-url="null"
    :background-video-source="backgroundVideoSource"
    :hero-background-url="heroBackgroundUrl"
    :hero-background-portrait-url="heroBackgroundPortraitUrl"
    :hero-background-landscape-url="heroBackgroundLandscapeUrl"
    :is-background-animated="props.isBackgroundAnimated"
    :background-image-fit="props.backgroundImageFit"
    :use-catalog-banners-as-background="props.useCatalogBannersAsBackground"
    :security-mode="props.securityMode"
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
    :previous-playable-title="previousNavigableEpisode?.title ?? null"
    :next-playable-title="nextNavigableEpisode?.title ?? null"
    :previous-playable-item="previousPlayableItem"
    :next-playable-item="nextPlayableItem"
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
     :price="details?.price ?? null"
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

    <template #below>
      <MediaCardCollection
        v-if="showRecommendations"
        ref="recommendationsRailRef"
        :items="recommendationItems"
        :label="t('entry.recommendations')"
        mode="single-row"
        thumbnail-orientation="landscape"
        :thumbnail-image-fit="'cover'"
        :is-loading-more="isRecommendationsLoading || isRecommendationsLoadingMore"
        :have-more="hasMoreRecommendations"
        :load-more-error-message="recommendationsErrorMessage"
        :initial-loading-message="t('entry.loadingRecommendationsMessage')"
        :on-load-more="handleLoadMoreRecommendations"
        :show-service-logo="true"
        @select="handleRecommendationSelect"
      />
    </template>
  </EntryDetails>
</template>
