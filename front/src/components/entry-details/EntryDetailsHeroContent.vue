<script setup lang="ts">
import { computed } from 'vue'

import type { EntryPlayer } from '@/types/entry'
import type { EntryPlayerLanguageOption } from '@/composables/entry-details/entryVideoPlayer'
import type { ResolvedPlayerMediaSource } from '@/services/players'

import VideoPlayer from '@/components/media/VideoPlayer.vue'
import EntryDetailsHeroHeader from './EntryDetailsHeroHeader.vue'
import { textToHtml } from '@/services/textUtils'

/**
 * Props accepted by the header and player section rendered in the main entry details column.
 */
interface Props {
  displayTitle: string
  alternativeTitleLabel: string | null
  selectedPlayableTitle: string | null
  showAdjacentNavigation: boolean
  hasPreviousPlayable: boolean
  hasNextPlayable: boolean
  showBookmarkAction: boolean
  isBookmarked: boolean
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
  showAutoplayToggle: boolean
  isAutoplayEnabled: boolean
}

/**
 * Determines the active media source based on the current player mode.
 */
const activeMediaSource = computed(() => {
  if (props.showMediaPlayer) {
    return props.mediaSource
  }

  if (props.showTrailerPlayer) {
    return props.trailerMediaSource
  }

  return null
})

/** Component props without defaults. */
const props = defineProps<Props>()
const emit = defineEmits<{
  /** Emitted when navigating to adjacent playable entries. */
  'step-playable': [offset: -1 | 1]
  /** Emitted when navigating to adjacent playable entries from the player control bar. */
  'step-playable-autoplay': [offset: -1 | 1]
  /** Emitted when the active language key changes. */
  'update:active-language-key': [value: string | null]
  /** Emitted to remember the current language selection. */
  'remember-current-language': []
  /** Emitted when the active player ID changes. */
  'update:active-player-id': [value: string | null]
  /** Emitted to remember the current player selection. */
  'remember-current-player': []
  /** Emitted when the bookmark toggle is clicked. */
  'toggle-bookmark': []
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

/**
 * Indicates whether the selected playable item header should be shown above the player.
 */
const showPlayableNavigation = computed(() =>
  props.showAdjacentNavigation &&
  Boolean(props.selectedPlayableTitle && (props.showTrailerPlayer || props.showMediaPlayer)),
)

/**
 * Requests selecting the previous or next playable item.
 *
 * @param offset Relative episode offset to apply.
 */
function handlePlayableStep(offset: -1 | 1) {
  emit('step-playable', offset)
}

/**
 * Toggles the bookmark stored for the current entry.
 */
function handleBookmarkToggle() {
  emit('toggle-bookmark')
}

/**
 * Requests selecting the previous or next playable item with autoplay enabled,
 * triggered by the player control bar navigation buttons.
 *
 * @param offset Relative episode offset to apply.
 */
function handleAutoplayStep(offset: -1 | 1) {
  emit('step-playable-autoplay', offset)
}
</script>

<template>
  <header id="player-section" class="entry-details__header">
    <div class="entry-details__headline">

      <EntryDetailsHeroHeader
        :display-title="displayTitle"
        :alternative-title-label="alternativeTitleLabel"
        :selected-playable-title="selectedPlayableTitle"
        :show-adjacent-navigation="showPlayableNavigation"
        :has-previous-playable="hasPreviousPlayable"
        :has-next-playable="hasNextPlayable"
        :show-bookmark-action="showBookmarkAction"
        :is-bookmarked="isBookmarked"
        @step-playable="handlePlayableStep"
        @toggle-bookmark="handleBookmarkToggle"
      />

<VideoPlayer
        :display-title="displayTitle"
        :show-trailer-player="showTrailerPlayer"
        :show-media-player="showMediaPlayer"
        :media-source="activeMediaSource"
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
        :poster="trailerPosterUrl"
        :media-poster-url="mediaPosterUrl"
        :media-overlay-logo-url="mediaOverlayLogoUrl"
        :initial-playback-time="initialPlaybackTime"
        :media-autoplay="mediaAutoplay"
        :prefer-persisted-media-surface="preferPersistedMediaSurface"
        :show-episode-autoplay-toggle="showAutoplayToggle"
        :is-episode-autoplay-enabled="isAutoplayEnabled"
        :show-video-navigation-controls="showAdjacentNavigation"
        :has-previous-video="hasPreviousPlayable"
        :has-next-video="hasNextPlayable"
        @update:active-language-key="emit('update:active-language-key', $event)"
        @remember-current-language="emit('remember-current-language')"
        @update:active-player-id="emit('update:active-player-id', $event)"
        @remember-current-player="emit('remember-current-player')"
        @update:playback-progress="(value: number | null, duration: number | null) => emit('update:playback-progress', value, duration)"
        @update:is-episode-autoplay-enabled="emit('update:is-autoplay-enabled', $event)"
        @playback-started="emit('playback-started', $event)"
        @playback-ended="emit('playback-ended')"
        @source-error="emit('source-error')"
        @navigate-video="handleAutoplayStep"
      />

      <p
        class="entry-details__description"
        v-html="textToHtml(displayDescription)"
      />
    </div>
  </header>
</template>

<style scoped>
.entry-details__header {
  display: grid;
  gap: 18px;
}

.entry-details__headline {
  display: grid;
  gap: 12px;
}

.entry-details__description {
  max-width: 880px;
  color: var(--text-primary);
  font-size: 1rem;
  line-height: 1.62;
  text-shadow: var(--text-shadow-primary);
}
</style>
