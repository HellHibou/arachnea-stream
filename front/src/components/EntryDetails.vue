<script setup lang="ts">
import { computed, onUnmounted, watch } from 'vue'
import Background from './Background.vue'
import EntryDetailsHeroContent from './entry-details/EntryDetailsHeroContent.vue'
import EntryDetailsMetadata from './entry-details/EntryDetailsMetadata.vue'
import EntryDetailsPosterPanel from './entry-details/EntryDetailsPosterPanel.vue'
import ScrollToTopButton from '@/components/ScrollToTopButton.vue'
import { useScrollToTop } from '@/composables/useScrollToTop'
import { APP_TITLE } from '@/constants'
import { useI18n } from '@/i18n'

import type { EntryPlayer } from '@/types/entry'
import type { EntryPlayerLanguageOption } from '@/composables/entry-details/entryVideoPlayer'
import type { ResolvedPlayerMediaSource } from '@/services/players'
import type { ThumbnailImageFit } from '@/types/media'

/**
  * Props accepted by the shared entry details shell.
  */
  interface Props {
    /** Error message to display when loading fails. */
    errorMessage: string | null
    /** Whether the component is currently loading content. */
    isLoading: boolean
    /** Whether the component has content to display. */
    hasContent: boolean
    /** Title to display during loading state. */
    loadingTitle?: string
    /** Description to display during loading state. */
    loadingDescription?: string
    /** URL of the background video to display. */
    backgroundVideoUrl: string | null
    /** URL of the hero background image to display. */
    heroBackgroundUrl: string | null
    /** URL of the portrait-oriented hero background image. */
    heroBackgroundPortraitUrl: string | null
    /** URL of the landscape-oriented hero background image. */
    heroBackgroundLandscapeUrl: string | null
    /** Whether the background should have animation effects. */
    isBackgroundAnimated?: boolean
    /** How the background image should fit its container. */
    backgroundImageFit?: ThumbnailImageFit
    /** Whether to use catalog banners as the background source. */
    useCatalogBannersAsBackground?: boolean
    /** Primary title to display for the entry. */
    displayTitle: string
    /** URL of the poster frame image to display. */
    posterFrameImageUrl: string | null
    /** Whether the poster frame should use contain sizing. */
    posterFrameUsesContain: boolean
    /** Whether to show the trailer action button. */
    showTrailerAction: boolean
    /** Label for the trailer action button. */
    trailerActionLabel: string
    /** URL to the entry detail page. */
    entryUrl: string | null
    /** Source identifier for the entry. */
    source: string | null
    /** Alternative title label to display. */
    alternativeTitleLabel: string | null
    /** Currently selected playable title. */
    selectedPlayableTitle: string | null
    /** Whether to show adjacent navigation controls. */
    showAdjacentNavigation?: boolean
    /** Whether there is a previous playable item. */
    hasPreviousPlayable?: boolean
    /** Whether there is a next playable item. */
    hasNextPlayable?: boolean
    /** Whether to show the bookmark action button. */
    showBookmarkAction?: boolean
    /** Whether the entry is currently bookmarked. */
    isBookmarked?: boolean
    /** Whether to show the trailer player. */
    showTrailerPlayer: boolean
    /** Whether to show the main media player. */
    showMediaPlayer: boolean
    /** Trailer media source for the player. */
    trailerMediaSource: ResolvedPlayerMediaSource | null
    /** Main media source for the player. */
    mediaSource: ResolvedPlayerMediaSource | null
    /** URL to open the media in a new tab. */
    mediaOpenUrl: string | null
    /** Whether the media player is currently loading. */
    isMediaPlayerLoading: boolean
    /** Error message from the media player. */
    mediaPlayerErrorMessage: string | null
    /** Whether to show player controls. */
    showPlayerControls: boolean
    /** Whether to show the language selector. */
    showLanguageSelector: boolean
    /** Whether to show the player selector. */
    showPlayerSelector: boolean
    /** Available language options for the player. */
    availableLanguages: EntryPlayerLanguageOption[]
    /** Currently active language key. */
    activeLanguageKey: string | null
    /** Filtered list of available players. */
    filteredPlayers: EntryPlayer[]
    /** Currently active player ID. */
    activePlayerId: string | null
    /** URL of the media poster image. */
    mediaPosterUrl: string | null
    /** URL of the trailer poster image. */
    trailerPosterUrl: string | null
    /** URL of the media overlay logo. */
    mediaOverlayLogoUrl: string | null
    /** Description text to display for the entry. */
    displayDescription: string
    /** Initial playback time in seconds. */
    initialPlaybackTime: number | null
    /** Whether media should autoplay. */
    mediaAutoplay: boolean
    /** Whether to prefer persisted media surface. */
    preferPersistedMediaSurface: boolean
    /** Whether to show autoplay toggle control. */
    showAutoplayToggle?: boolean
    /** Whether autoplay is currently enabled. */
    isAutoplayEnabled?: boolean
    /** Whether to display content in full width mode. */
    isFullWidthContent?: boolean
    /** List of metadata badges to display. */
    metadataBadges: string[]
    /** List of topic chips to display. */
    topicChips: string[]
    /** Genre text to display. */
    genreText: string | null
    /** Year label to display. */
    yearLabel: string | null
    /** Release date label to display. */
    displayReleaseDateLabel: string | null
    /** Expiration date label to display. */
    displayExpireLabel: string | null
    /** Content advisor label to display. */
    contentAdvisorLabel: string | null
    /** Audio language label to display. */
    audioLanguageLabel: string | null
    /** Subtitle language label to display. */
    subtitleLanguageLabel: string | null
    /** Duration label to display. */
    displayDurationLabel: string | null
    /** Casting text to display. */
    castingText: string | null
    /** Director text to display. */
    directorText: string | null
    /** Score to display for the entry. */
    score: number | null
 }

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
   isBackgroundAnimated: false,
   backgroundImageFit: 'contain',
   showAdjacentNavigation: false,
   hasPreviousPlayable: false,
   hasNextPlayable: false,
   showBookmarkAction: true,
   isBookmarked: false,
   showAutoplayToggle: false,
   isAutoplayEnabled: false,
   isFullWidthContent: false,
   heroBackgroundPortraitUrl: null,
   heroBackgroundLandscapeUrl: null,
   useCatalogBannersAsBackground: true,
  })
/** Internationalization utilities. */
const { t } = useI18n()

/** Resolved loading title with fallback to translated default. */
const loadingTitle = computed(() => props.loadingTitle ?? t('entry.loadingTitle'))
/** Resolved loading description with fallback to translated default. */
const loadingDescription = computed(() => props.loadingDescription ?? t('entry.loadingMessage'))

/**
 * Builds the browser tab title from the entry title, the selected playable title
 * when available, and the application title.
 */
const pageTitle = computed(() => {
  let parts = props.displayTitle

  const selectedPlayableTitle = props.selectedPlayableTitle?.trim()
  if (selectedPlayableTitle && selectedPlayableTitle !== props.displayTitle) {
    parts += ' - ' + selectedPlayableTitle;
  }

  parts += ' - ' + APP_TITLE;
  return parts;
})

/** Updates the browser tab title whenever the page title changes. */
watch(pageTitle, (title) => {
  document.title = title
}, { immediate: true })

/** Restores the default application title when the component is unmounted. */
onUnmounted(() => {
  document.title = APP_TITLE
})

/** Whether the autoplay toggle should be shown based on props. */
const resolvedShowAutoplayToggle = computed(() =>
  props.showAutoplayToggle && (props.hasPreviousPlayable || props.hasNextPlayable)
)

/** Scroll-to-top button state and handler. */
const { showScrollToTop, scrollToTop } = useScrollToTop({
  target: 'title-section',
  shouldShow: () => {
    const playerSection = document.getElementById('player-section')

    if (!playerSection) {
      return window.scrollY > 400
    }

    return playerSection.getBoundingClientRect().bottom < 0 && window.scrollY > 200
  },
})

const emit = defineEmits<{
  /** Emitted when navigating to adjacent playable entries. */
  'step-playable': [offset: -1 | 1]
  /** Emitted when navigating to adjacent playable entries from the player control bar. */
  'step-playable-autoplay': [offset: -1 | 1]
  /** Emitted when the bookmark toggle is clicked. */
  'toggle-bookmark': []
  /** Emitted when the trailer toggle is clicked. */
  'toggle-trailer': []
  /** Emitted when the active language key changes. */
  'update:active-language-key': [value: string | null]
  /** Emitted to remember the current language selection. */
  'remember-current-language': []
  /** Emitted when the active player ID changes. */
  'update:active-player-id': [value: string | null]
  /** Emitted to remember the current player selection. */
  'remember-current-player': []
  /** Emitted when playback progress updates. */
  'update:playback-progress': [value: number | null, duration: number | null]
  /** Emitted when autoplay enabled state changes. */
  'update:is-autoplay-enabled': [value: boolean]
  /** Emitted when media playback starts. */
  'playback-started': [sourceUrl: string | null]
  /** Emitted when media playback ends. */
  'playback-ended': []
  /** Emitted when the active Video.js source fails. */
  'source-error': []
}>()
</script>

<template>
    <section class="entry-details">
      <Background
        :video-url="props.backgroundVideoUrl"
        :image-url="null"
        :image-portrait-url="props.useCatalogBannersAsBackground ? props.heroBackgroundPortraitUrl : null"
        :image-landscape-url="props.useCatalogBannersAsBackground ? props.heroBackgroundLandscapeUrl : null"
        :is-animated="props.isBackgroundAnimated"
        :image-fit="props.backgroundImageFit"
      />

     <article
       class="entry-details__shell"
       :class="{ 'entry-details__shell--loading': props.isLoading }"
     >
       <div v-if="props.errorMessage" class="entry-details__state">
         <h2 class="entry-details__state-title">{{ t('catalog.loadingErrorTitle') }}</h2>
         <p class="entry-details__state-copy">
           {{ props.errorMessage }}
         </p>
       </div>

       <div v-else-if="props.isLoading && !props.hasContent" class="entry-details__state">
         <h2 class="entry-details__state-title">{{ loadingTitle }}</h2>
         <p class="entry-details__state-copy">
           {{ loadingDescription }}
         </p>
       </div>

       <div
         v-else-if="props.hasContent"
         class="entry-details__content"
         :class="{ 'entry-details__content--full-width': props.isFullWidthContent }"
       >
         <EntryDetailsPosterPanel
           v-if="!props.isFullWidthContent"
           :display-title="props.displayTitle"
           :poster-frame-image-url="props.posterFrameImageUrl"
           :poster-frame-uses-contain="props.posterFrameUsesContain"
           :show-trailer-action="props.showTrailerAction"
           :trailer-action-label="props.trailerActionLabel"
           :entry-url="props.entryUrl"
           :source="props.source"
           @toggle-trailer="emit('toggle-trailer')"
         />

         <div
           v-if="!props.isFullWidthContent"
           class="entry-details__body"
         >
           <EntryDetailsHeroContent
             ref="playerSectionRef"
             :display-title="props.displayTitle"
             :alternative-title-label="props.alternativeTitleLabel"
             :selected-playable-title="props.selectedPlayableTitle"
             :show-adjacent-navigation="props.showAdjacentNavigation"
             :has-previous-playable="props.hasPreviousPlayable"
             :has-next-playable="props.hasNextPlayable"
             :show-bookmark-action="props.showBookmarkAction"
             :is-bookmarked="props.isBookmarked"
             :show-trailer-player="props.showTrailerPlayer"
             :show-media-player="props.showMediaPlayer"
             :trailer-media-source="props.trailerMediaSource"
             :media-source="props.mediaSource"
             :media-open-url="props.mediaOpenUrl"
             :is-media-player-loading="props.isMediaPlayerLoading"
             :media-player-error-message="props.mediaPlayerErrorMessage"
             :show-player-controls="props.showPlayerControls"
             :show-language-selector="props.showLanguageSelector"
             :show-player-selector="props.showPlayerSelector"
             :available-languages="props.availableLanguages"
             :active-language-key="props.activeLanguageKey"
             :filtered-players="props.filteredPlayers"
             :active-player-id="props.activePlayerId"
             :media-poster-url="props.mediaPosterUrl"
             :trailer-poster-url="props.trailerPosterUrl"
             :media-overlay-logo-url="props.mediaOverlayLogoUrl"
             :display-description="props.displayDescription"
             :initial-playback-time="props.initialPlaybackTime"
             :media-autoplay="props.mediaAutoplay"
             :prefer-persisted-media-surface="props.preferPersistedMediaSurface"
              :show-autoplay-toggle="resolvedShowAutoplayToggle"
              :is-autoplay-enabled="props.isAutoplayEnabled"
              @step-playable="emit('step-playable', $event)"
              @step-playable-autoplay="emit('step-playable-autoplay', $event)"
             @toggle-bookmark="emit('toggle-bookmark')"
             @update:active-language-key="emit('update:active-language-key', $event)"
             @remember-current-language="emit('remember-current-language')"
             @update:active-player-id="emit('update:active-player-id', $event)"
             @remember-current-player="emit('remember-current-player')"
             @update:playback-progress="(value: number | null, duration: number | null) => emit('update:playback-progress', value, duration)"
             @update:is-autoplay-enabled="emit('update:is-autoplay-enabled', $event)"
             @playback-started="emit('playback-started', $event)"
             @playback-ended="emit('playback-ended')"
             @source-error="emit('source-error')"
           />

           <EntryDetailsMetadata
             :metadata-badges="props.metadataBadges"
             :topic-chips="props.topicChips"
             :genre-text="props.genreText"
             :year-label="props.yearLabel"
             :display-release-date-label="props.displayReleaseDateLabel"
             :display-expire-label="props.displayExpireLabel"
             :content-advisor-label="props.contentAdvisorLabel"
             :audio-language-label="props.audioLanguageLabel"
             :subtitle-language-label="props.subtitleLanguageLabel"
             :display-duration-label="props.displayDurationLabel"
             :casting-text="props.castingText"
             :director-text="props.directorText"
             :score="props.score"
           />

           <slot />
         </div>

         <div
           v-else
           class="entry-details__list-only-content"
         >
           <slot />
         </div>
       </div>
     </article>

     <ScrollToTopButton
       v-if="showScrollToTop"
       @click="scrollToTop"
     />
   </section>
 </template>

<style scoped>
.entry-details {
   position: relative;
   z-index: 1;
 }

.entry-details__shell {
   position: relative;
   z-index: 1;
   overflow: hidden;
   border-radius: 0;
   background: var(--bg-transparent);
   box-shadow: none;
   isolation: isolate;
 }

.entry-details__shell--loading {
   min-height: 320px;
 }

.entry-details__content,
.entry-details__state {
   position: relative;
   z-index: 1;
 }

.entry-details__content {
   display: grid;
   grid-template-columns: minmax(220px, 400px) minmax(0, 1fr);
   gap: 28px;
   padding: 28px;
 }

.entry-details__content--full-width {
   grid-template-columns: 1fr;
 }

.entry-details__list-only-content {
   display: grid;
   gap: 28px;
 }

.entry-details__state {
   display: grid;
   gap: 10px;
   min-height: 260px;
   padding: 28px;
   align-content: center;
   text-align: center;
 }

.entry-details__state-title {
   font-size: clamp(1.6rem, 1.42rem + 0.8vw, 2.2rem);
   font-weight: 800;
 }

.entry-details__state-copy {
   max-width: 580px;
   margin: 0 auto;
   color: var(--text-secondary);
 }

.entry-details__body {
   display: grid;
   gap: 18px;
   min-width: 0;
   align-content: start;
 }

@media (max-width: 1120px) {
   .entry-details__content {
     grid-template-columns: 1fr;
   }
 }

@media (max-width: 720px) {
   .entry-details__content,
   .entry-details__state {
     padding: 18px;
   }
 }

</style>
