<script setup lang="ts">
import { computed, nextTick, ref, shallowRef, watch } from 'vue'

import EntryDetails from './EntryDetails.vue'
import EntryDetailsCatalogSection from './entry-details/EntryDetailsCatalogSection.vue'
import { entryVideoPlayer } from '@/composables/entry-details/entryVideoPlayer'
import {
  getLivePlayers,
  listLiveMediaItems,
  resolvePlayerStream,
} from '@/services/rustify'
import {
  resolveBackendStreamMediaSource,
  resolvePlayerMediaSource,
  type ResolvedPlayerMediaSource,
} from '@/services/players'
import { useI18n } from '@/i18n'
import type { EntryDetails as EntryDetailsModel, EntryPlayableItem, EntryPlayer } from '@/types/entry'
import type { MediaItem, ThumbnailImageFit } from '@/types/media'
import { MSG_LIVE_TV, MSG_LIVE, MSG_LIVE_PLAYING } from '@/i18n/index.ts';

/**
  * Props accepted by the live details page.
  */
 interface Props {
   /**
    * Initial live source restored from a direct route.
    * @default null
    */
   initialSource?: string | null
   /**
    * Initial live channel restored from a direct route.
    * @default null
    */
   initialChannel?: string | null
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
    * Indicates whether the catalog background should be displayed.
    * @default true
    */
   useCatalogBannersAsBackground?: boolean
 }

/**
 * Scrolls back to the player title once the DOM has applied the latest live selection.
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

const props = withDefaults(defineProps<Props>(), {
   initialSource: null,
   initialChannel: null,
   isBackgroundAnimated: false,
   backgroundImageFit: 'contain',
   useCatalogBannersAsBackground: true,
  })
const emit = defineEmits<{
  'select-live': [item: MediaItem]
}>()
const { t } = useI18n()

const liveItems = ref<MediaItem[]>([])
const livePlayersByItemId = shallowRef<Record<string, EntryPlayer[]>>({})
const selectedLiveId = shallowRef<string | null>(null)
const isLoading = shallowRef(false)
const errorMessage = shallowRef<string | null>(null)
const isLiveSelectionLoading = shallowRef(false)
const liveSelectionErrorMessage = shallowRef<string | null>(null)
const resolvedLiveMediaSource = shallowRef<ResolvedPlayerMediaSource | null>(null)
const resolvedLiveMediaOpenUrl = shallowRef<string | null>(null)
const isResolvedLiveMediaLoading = shallowRef(false)
const resolvedLiveMediaErrorMessage = shallowRef<string | null>(null)

let latestListRequestId = 0
let latestSelectionRequestId = 0
let latestMediaResolutionRequestId = 0

/**
 * Exposes the live item currently selected in the page.
 */
const selectedLiveItem = computed<MediaItem | null>(() => {
  if (!selectedLiveId.value) {
    return null
  }

  return liveItems.value.find((item) => item.id === selectedLiveId.value) ?? null
})

/**
 * Exposes the players already loaded for the selected live item.
 */
const selectedLivePlayers = computed<EntryPlayer[]>(() => {
  const selectedItemId = selectedLiveItem.value?.id
  if (!selectedItemId) {
    return []
  }

  return livePlayersByItemId.value[selectedItemId] ?? []
})

/**
 * Exposes the selected live item as one playable item compatible with the shared player flow.
 */
const selectedPlayableItem = computed<EntryPlayableItem | null>(() => {
  const liveItem = selectedLiveItem.value
  if (!liveItem) {
    return null
  }

  return {
    id: liveItem.id,
    link: liveItem.webUrl ?? liveItem.entryUrl,
    players: selectedLivePlayers.value,
    seasonName: null,
    title: liveItem.title?.trim() || null,
    description: liveItem.overview ?? MSG_LIVE_TV,
    releaseDateLabel: liveItem.releaseDateLabel,
    expireLabel: liveItem.expireLabel,
    durationLabel: liveItem.durationLabel,
    previewUrl: liveItem.imagePosterUrl ?? liveItem.imageLandscapeUrl,
  }
})

/**
 * Exposes the synthetic details payload consumed by the shared details shell and player.
 */
const details = computed<EntryDetailsModel | null>(() => {
  const liveItem = selectedLiveItem.value
  if (!liveItem) {
    return null
  }

  return {
    source: liveItem.source ?? '',
    entryUrl: liveItem.webUrl ?? liveItem.entryUrl,
    title: liveItem.title?.trim() || MSG_LIVE,
    alternativeTitleLabel: liveItem.alternativeTitleLabel?.trim() || null,
    trailerUrl: null,
    players: selectedLivePlayers.value,
    description: liveItem.overview ?? MSG_LIVE_PLAYING,
    imagePosterUrl: liveItem.imagePosterUrl,
    imagePortraitUrl: liveItem.imagePortraitUrl,
    imageLandscapeUrl: liveItem.imageLandscapeUrl,
    logoUrl: null,
    heroImageUrl: liveItem.imageLandscapeUrl ?? liveItem.imagePosterUrl,
    yearLabel: null,
    releaseDateLabel: liveItem.releaseDateLabel,
    expireLabel: liveItem.expireLabel,
    durationLabel: liveItem.durationLabel,
    seasonCountLabel: null,
    audioLanguageLabel: liveItem.audioLabel,
    subtitleLanguageLabel: null,
    contentAdvisorLabel: null,
    themeLabels: liveItem.themeLabels,
    genreLabels: [],
    castingLabels: [],
    directorLabels: [],
    seasons: [],
    episodes: [],
  }
})

const {
  activeLanguageKey,
  availableLanguages,
  filteredPlayers,
  activePlayerId,
  trailerUrl,
  trailerMediaSource,
  showTrailerPlayer,
  selectedPlayableTitle,
  showTrailerAction,
  trailerActionLabel,
  showLanguageSelector,
  showPlayerSelector,
  showPlayerControls,
  handleTrailerToggle,
  rememberCurrentLanguage,
  rememberCurrentPlayer,
} = entryVideoPlayer({
  details,
  selectedPlayableItem,
})

/**
 * Exposes the live player currently selected in the shared player controls.
 */
const selectedLivePlayer = computed<EntryPlayer | null>(() =>
  filteredPlayers.value.find((player) => player.id === activePlayerId.value) ??
  filteredPlayers.value[0] ??
  null,
)

/**
 * Exposes the media loading state including the live player lookup.
 */
const combinedMediaPlayerLoading = computed(
  () => isLiveSelectionLoading.value || isResolvedLiveMediaLoading.value,
)

/**
 * Exposes the media player error message including live item lookup failures.
 */
const combinedMediaPlayerErrorMessage = computed(
  () => liveSelectionErrorMessage.value ?? resolvedLiveMediaErrorMessage.value,
)

/**
 * Keeps the shared player surface visible while the selected live is being loaded.
 */
const shouldShowMediaPlayer = computed(
  () => selectedLivePlayers.value.length > 0 || Boolean(resolvedLiveMediaSource.value) || combinedMediaPlayerLoading.value || Boolean(combinedMediaPlayerErrorMessage.value),
)

/**
 * Exposes the poster used by the shared player shell.
 */
const mediaPosterUrl = computed(() =>
  selectedLiveItem.value?.imagePosterUrl ?? selectedLiveItem.value?.imageLandscapeUrl ?? null,
)

/**
 * Exposes the background image reused by the shared details shell.
 */
const heroBackgroundUrl = computed(() =>
  selectedLiveItem.value?.imagePortraitUrl ?? selectedLiveItem.value?.imagePosterUrl ?? null,
)

/**
 * Exposes the portrait-oriented background image for portrait viewport.
 */
const heroBackgroundPortraitUrl = computed(() =>
  selectedLiveItem.value?.imagePortraitUrl ?? selectedLiveItem.value?.imagePosterUrl ?? null,
)

/**
 * Exposes the landscape-oriented background image for landscape viewport.
 */
const heroBackgroundLandscapeUrl = computed(() =>
  selectedLiveItem.value?.imageLandscapeUrl ?? null,
)

/**
 * Stores the selected language in the shared player state.
 *
 * @param value Language key selected in the player controls.
 */
function handleActiveLanguageKeyUpdate(value: string | null) {
  activeLanguageKey.value = value
}

/**
 * Stores the selected player in the shared player state.
 *
 * @param value Player identifier selected in the player controls.
 */
function handleActivePlayerIdUpdate(value: string | null) {
  activePlayerId.value = value
}

/**
 * Resolves the currently selected live player into one media source consumable by the shared shell.
 *
 * @param liveItem Live entry currently selected in the details page.
 * @param player Embedded player selected for the current live.
 */
async function resolveSelectedLiveMedia(
  liveItem: MediaItem | null,
  player: EntryPlayer | null,
) {
  const requestId = ++latestMediaResolutionRequestId

  resolvedLiveMediaSource.value = null
  resolvedLiveMediaOpenUrl.value = player?.embedLink ?? null
  resolvedLiveMediaErrorMessage.value = null

  if (!liveItem?.source || !player) {
    return
  }

  isResolvedLiveMediaLoading.value = true

  try {
    const nextMediaSource = player.resolver
      ? await resolvePlayerStream(liveItem.source, player).then((resolvedStream) =>
          resolvedStream
            ? resolveBackendStreamMediaSource(
                resolvedStream.streamUrl,
                resolvedStream.manifestType,
                resolvedStream.licenseUrl,
                resolvedStream.licenseHeaders,
              )
            : null,
        )
      : resolvePlayerMediaSource(player.embedLink)

    if (requestId !== latestMediaResolutionRequestId) {
      return
    }

    if (!nextMediaSource) {
      resolvedLiveMediaErrorMessage.value = t('entry.videoUnavailable')
      return
    }

    resolvedLiveMediaSource.value = nextMediaSource
  } catch (error) {
    if (requestId !== latestMediaResolutionRequestId) {
      return
    }

    resolvedLiveMediaErrorMessage.value =
      error instanceof Error ? error.message : t('errors.livePlayer')
  } finally {
    if (requestId === latestMediaResolutionRequestId) {
      isResolvedLiveMediaLoading.value = false
    }
  }
}

/**
 * Loads the live list from the backend without selecting any live item.
 */
async function loadLiveItems() {
  const requestId = ++latestListRequestId

  isLoading.value = true
  errorMessage.value = null

  try {
    const nextLiveItems = await listLiveMediaItems()

    if (requestId !== latestListRequestId) {
      return
    }

    liveItems.value = nextLiveItems

    if (nextLiveItems.length === 0) {
      errorMessage.value = t('live.none')
      selectedLiveId.value = null
      return
    }

    selectedLiveId.value = null
  } catch (error) {
    if (requestId !== latestListRequestId) {
      return
    }

    liveItems.value = []
    selectedLiveId.value = null
    errorMessage.value =
      error instanceof Error ? error.message : t('errors.live')
  } finally {
    if (requestId === latestListRequestId) {
      isLoading.value = false
    }
  }
}

/**
 * Loads the selected live players when needed.
 *
 * @param liveItem Live item selected from the bottom list.
 */
async function selectLiveItem(liveItem: MediaItem, options: { shouldEmit?: boolean } = {}) {
   if (!liveItem.source || !liveItem.entryUrl) {
     return
   }

   if (options.shouldEmit ?? true) {
     emit('select-live', liveItem)
   }

   selectedLiveId.value = liveItem.id
   liveSelectionErrorMessage.value = null
   resolvedLiveMediaSource.value = null
   resolvedLiveMediaOpenUrl.value = null
   resolvedLiveMediaErrorMessage.value = null

   const cachedPlayers = livePlayersByItemId.value[liveItem.id]
   if (cachedPlayers && cachedPlayers.length > 0) {
     const cachedPlayer = cachedPlayers.find((player) => player.id === activePlayerId.value) ?? cachedPlayers[0] ?? null
     if (cachedPlayer && cachedPlayer.id !== activePlayerId.value) {
       activePlayerId.value = cachedPlayer.id
     }

     await resolveSelectedLiveMedia(liveItem, cachedPlayer)
     scrollToTitleSection()
     return
   }

   const requestId = ++latestSelectionRequestId
   isLiveSelectionLoading.value = true

   try {
     const players = await getLivePlayers(liveItem.source, liveItem.entryUrl)

     if (requestId !== latestSelectionRequestId) {
       return
     }

     if (players.length === 0) {
       liveSelectionErrorMessage.value = t('live.noPlayer')
       return
     }

     livePlayersByItemId.value = {
       ...livePlayersByItemId.value,
       [liveItem.id]: players,
     }

     const initialPlayer = players.find((player) => player.id === activePlayerId.value) ?? players[0] ?? null
     if (initialPlayer && initialPlayer.id !== activePlayerId.value) {
       activePlayerId.value = initialPlayer.id
     }

     await resolveSelectedLiveMedia(liveItem, initialPlayer)
     scrollToTitleSection()
   } catch (error) {
     if (requestId !== latestSelectionRequestId) {
       return
     }

     liveSelectionErrorMessage.value =
       error instanceof Error ? error.message : t('errors.livePlayer')
   } finally {
     if (requestId === latestSelectionRequestId) {
       isLiveSelectionLoading.value = false
     }
   }
  }

watch(
   [
     () => props.initialSource,
     () => props.initialChannel,
     () => JSON.stringify(liveItems.value.map((item) => [item.source, item.entryUrl])),
   ],
   ([initialSource, initialChannel]) => {
     const source = initialSource?.trim()
     const channel = initialChannel?.trim()

     if (!source || !channel) {
       selectedLiveId.value = null
       liveSelectionErrorMessage.value = null
       resolvedLiveMediaSource.value = null
       resolvedLiveMediaOpenUrl.value = null
       resolvedLiveMediaErrorMessage.value = null
       return
     }

     const matchingLiveItem = liveItems.value.find(
       (item) => item.source === source && item.entryUrl === channel,
     )

     if (!matchingLiveItem) {
       selectedLiveId.value = null
       liveSelectionErrorMessage.value = null
       resolvedLiveMediaSource.value = null
       resolvedLiveMediaOpenUrl.value = null
       resolvedLiveMediaErrorMessage.value = null
       return
     }

     if (selectedLiveId.value === matchingLiveItem.id) {
       return
     }

     void selectLiveItem(matchingLiveItem, { shouldEmit: false })
   },
   { immediate: true },
  )

watch(
   [() => selectedLiveItem.value?.id ?? null, () => selectedLivePlayer.value?.id ?? null],
   ([liveItemId, playerId], [previousLiveItemId, previousPlayerId]) => {
     if (!liveItemId || !playerId) {
       return
     }

     if (liveItemId === previousLiveItemId && playerId === previousPlayerId) {
       return
     }

     void resolveSelectedLiveMedia(selectedLiveItem.value, selectedLivePlayer.value)
     scrollToTitleSection()
   },
  )

void loadLiveItems()
</script>

<template>
<EntryDetails
     :error-message="errorMessage"
     :is-loading="isLoading"
     :has-content="liveItems.length > 0"
     :loading-description="t('live.loadingMessage')"
     :background-video-url="null"
     :hero-background-url="heroBackgroundUrl"
     :hero-background-portrait-url="heroBackgroundPortraitUrl"
     :hero-background-landscape-url="heroBackgroundLandscapeUrl"
     :is-background-animated="props.isBackgroundAnimated"
     :background-image-fit="props.backgroundImageFit"
     :use-catalog-banners-as-background="props.useCatalogBannersAsBackground"
     :display-title="details?.title ?? t('live.title')"
    :poster-frame-image-url="mediaPosterUrl"
    :poster-frame-uses-contain="false"
    :show-trailer-action="showTrailerAction"
    :trailer-action-label="trailerActionLabel"
    :entry-url="details?.entryUrl ?? null"
    :source="selectedLiveItem?.source ?? null"
    :alternative-title-label="details?.alternativeTitleLabel ?? null"
    :selected-playable-title="selectedPlayableTitle"
    :show-bookmark-action="false"
    :show-trailer-player="showTrailerPlayer"
    :show-media-player="shouldShowMediaPlayer"
    :trailer-media-source="trailerMediaSource"
    :media-source="resolvedLiveMediaSource"
    :media-open-url="resolvedLiveMediaOpenUrl"
    :is-media-player-loading="combinedMediaPlayerLoading"
    :media-player-error-message="combinedMediaPlayerErrorMessage"
    :show-player-controls="showPlayerControls"
    :show-language-selector="showLanguageSelector"
    :show-player-selector="showPlayerSelector"
    :available-languages="availableLanguages"
    :active-language-key="activeLanguageKey"
    :filtered-players="filteredPlayers"
    :active-player-id="activePlayerId"
    :media-poster-url="mediaPosterUrl"
    :trailer-poster-url="null"
    :media-overlay-logo-url="null"
    :display-description="details?.description ?? t('live.prompt')"
    :is-full-width-content="!selectedLiveItem"
    :initial-playback-time="null"
    :media-autoplay="true"
    :prefer-persisted-media-surface="false"
    :show-autoplay-toggle="false"
    :is-autoplay-enabled="false"
    :metadata-badges="[t('live.direct')]"
    :topic-chips="details?.themeLabels ?? []"
    :genre-text="null"
    :year-label="null"
    :display-release-date-label="details?.releaseDateLabel ?? null"
    :display-expire-label="details?.expireLabel ?? null"
    :content-advisor-label="null"
    :audio-language-label="details?.audioLanguageLabel ?? null"
    :subtitle-language-label="null"
    :display-duration-label="details?.durationLabel ?? null"
    :casting-text="null"
    :director-text="null"
    @toggle-trailer="handleTrailerToggle"
    @update:active-language-key="handleActiveLanguageKeyUpdate"
    @remember-current-language="rememberCurrentLanguage"
    @update:active-player-id="handleActivePlayerIdUpdate"
    @remember-current-player="rememberCurrentPlayer"
  >
    <EntryDetailsCatalogSection
      :group-items="[]"
      selected-group-label=""
      :show-item-section="liveItems.length > 0"
      :group-error-message="null"
      :is-group-loading="false"
      :show-group-prompt="false"
      :show-empty-group-state="false"
      :displayed-items="liveItems"
      :displayed-item-label="t('live.streams')"
      :show-load-more-items="false"
      :is-loading-more-items="false"
      :item-section-label="t('live.streams')"
      :state-eyebrow="t('live.streams')"
      @select-item="selectLiveItem"
    />
  </EntryDetails>
</template>
