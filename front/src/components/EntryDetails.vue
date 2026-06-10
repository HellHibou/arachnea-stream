<script setup lang="ts">
import { computed } from 'vue'
import Background from './Background.vue'
import EntryDetailsHeroContent from './entry-details/EntryDetailsHeroContent.vue'
import EntryDetailsMetadata from './entry-details/EntryDetailsMetadata.vue'
import EntryDetailsPosterPanel from './entry-details/EntryDetailsPosterPanel.vue'
import ScrollToTopButton from '@/components/ScrollToTopButton.vue'
import { useScrollToTop } from '@/composables/useScrollToTop'
import { useI18n } from '@/i18n'

import type { EntryPlayer } from '@/types/entry'
import type { EntryPlayerLanguageOption } from '@/composables/entry-details/entryVideoPlayer'
import type { ResolvedPlayerMediaSource } from '@/services/players'
import type { ThumbnailImageFit } from '@/types/media'

/**
  * Props accepted by the shared entry details shell.
  */
  interface Props {
    errorMessage: string | null
    isLoading: boolean
    hasContent: boolean
    loadingTitle?: string
    loadingDescription?: string
    backgroundVideoUrl: string | null
    heroBackgroundUrl: string | null
    heroBackgroundPortraitUrl: string | null
    heroBackgroundLandscapeUrl: string | null
    isBackgroundAnimated?: boolean
    backgroundImageFit?: ThumbnailImageFit
    useCatalogBannersAsBackground?: boolean
    displayTitle: string
   posterFrameImageUrl: string | null
   posterFrameUsesContain: boolean
   showTrailerAction: boolean
   trailerActionLabel: string
   entryUrl: string | null
   source: string | null
   alternativeTitleLabel: string | null
   selectedPlayableTitle: string | null
   showAdjacentNavigation?: boolean
   hasPreviousPlayable?: boolean
   hasNextPlayable?: boolean
   showBookmarkAction?: boolean
   isBookmarked?: boolean
   showTrailerPlayer: boolean
   showMediaPlayer: boolean
   trailerMediaSource: ResolvedPlayerMediaSource | null
   mediaSource: ResolvedPlayerMediaSource | null
   mediaOpenUrl: string | null
   isMediaPlayerLoading: boolean
   mediaPlayerErrorMessage: string | null
   showPlayerControls: boolean
   showLanguageSelector: boolean
   showPlayerSelector: boolean
   availableLanguages: EntryPlayerLanguageOption[]
   activeLanguageKey: string | null
   filteredPlayers: EntryPlayer[]
   activePlayerId: string | null
   mediaPosterUrl: string | null
   trailerPosterUrl: string | null
   mediaOverlayLogoUrl: string | null
   displayDescription: string
   initialPlaybackTime: number | null
   mediaAutoplay: boolean
   preferPersistedMediaSurface: boolean
   showAutoplayToggle?: boolean
   isAutoplayEnabled?: boolean
   isFullWidthContent?: boolean
   metadataBadges: string[]
   topicChips: string[]
   genreText: string | null
   yearLabel: string | null
   displayReleaseDateLabel: string | null
   displayExpireLabel: string | null
   contentAdvisorLabel: string | null
   audioLanguageLabel: string | null
   subtitleLanguageLabel: string | null
   displayDurationLabel: string | null
   castingText: string | null
   directorText: string | null
   score: number | null
 }

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
const { t } = useI18n()

const loadingTitle = computed(() => props.loadingTitle ?? t('entry.loadingTitle'))
const loadingDescription = computed(() => props.loadingDescription ?? t('entry.loadingMessage'))

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
  'step-playable': [offset: -1 | 1]
  'toggle-bookmark': []
  'toggle-trailer': []
  'update:active-language-key': [value: string | null]
  'remember-current-language': []
  'update:active-player-id': [value: string | null]
  'remember-current-player': []
  'update:playback-progress': [value: number | null]
  'update:is-autoplay-enabled': [value: boolean]
  'playback-started': [sourceUrl: string | null]
  'playback-ended': []
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
             :show-autoplay-toggle="props.showAutoplayToggle"
             :is-autoplay-enabled="props.isAutoplayEnabled"
             @step-playable="emit('step-playable', $event)"
             @toggle-bookmark="emit('toggle-bookmark')"
             @update:active-language-key="emit('update:active-language-key', $event)"
             @remember-current-language="emit('remember-current-language')"
             @update:active-player-id="emit('update:active-player-id', $event)"
             @remember-current-player="emit('remember-current-player')"
             @update:playback-progress="emit('update:playback-progress', $event)"
             @update:is-autoplay-enabled="emit('update:is-autoplay-enabled', $event)"
             @playback-started="emit('playback-started', $event)"
             @playback-ended="emit('playback-ended')"
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
