<script setup lang="ts">
import { computed, nextTick, ref, shallowRef, watch } from 'vue'

import EntryDetails from './EntryDetails.vue'
import EntryDetailsCatalogSection from './entry-details/EntryDetailsCatalogSection.vue'
import {
  getLivePlayers,
  listLiveMediaItems,
  getStream,
} from '@/services/rustify'
import {
  resolveBackendStreamMediaSource,
  resolveIframeMediaSource,
  resolvePlayerMediaSource,
  type ResolvedPlayerMediaSource,
} from '@/services/players'
import { useI18n } from '@/i18n'
import type { EntryDetails as EntryDetailsModel, EntryPlayableItem, EntryPlayer, EntryResolvedPlayerStream } from '@/types/entry'
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

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
   initialSource: null,
   initialChannel: null,
   isBackgroundAnimated: false,
   backgroundImageFit: 'contain',
   useCatalogBannersAsBackground: true,
  })

const emit = defineEmits<{
  /** Emitted when a live media item is selected. */
  'select-live': [item: MediaItem]
}>()

/** Internationalization utilities. */
const { t } = useI18n()

/** List of available live media items. */
const liveItems = ref<MediaItem[]>([])
/** Map of live item IDs to their available players. */
const livePlayersByItemId = shallowRef<Record<string, EntryPlayer[]>>({})
/** ID of the currently selected live item. */
const selectedLiveId = shallowRef<string | null>(null)
/** Whether live items are currently loading. */
const isLoading = shallowRef(false)
/** Error message from live items loading. */
const errorMessage = shallowRef<string | null>(null)
/** Whether a live selection is currently loading. */
const isLiveSelectionLoading = shallowRef(false)
/** Error message from live selection loading. */
const liveSelectionErrorMessage = shallowRef<string | null>(null)
/** Resolved media source for the selected live player. */
const resolvedLiveMediaSource = shallowRef<ResolvedPlayerMediaSource | null>(null)
/** Resolved stream response retained to try alternative media URLs. */
const resolvedLiveStream = shallowRef<EntryResolvedPlayerStream | null>(null)
/** Active URL index within the resolved live stream. */
const activeResolvedLiveStreamIndex = shallowRef(0)
/** Open URL for the selected live media. */
const resolvedLiveMediaOpenUrl = shallowRef<string | null>(null)
/** Whether live media resolution is currently loading. */
const isResolvedLiveMediaLoading = shallowRef(false)
/** Error message from live media resolution. */
const resolvedLiveMediaErrorMessage = shallowRef<string | null>(null)
/** Selected player ID for the current live. */
const activePlayerId = shallowRef<string | null>(null)

/** Counter to track the latest live list request ID. */
let latestListRequestId = 0
/** Counter to track the latest live selection request ID. */
let latestSelectionRequestId = 0
/** Counter to track the latest media resolution request ID. */
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
    players:  { entries: selectedLivePlayers.value, source: liveItem.source ?? '' },
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
    players:  { entries: selectedLivePlayers.value, source: liveItem.source ?? '' },
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
    score: null,
  }
})

/**
 * Exposes the currently selected player from the available players.
 */
const selectedLivePlayer = computed<EntryPlayer | null>(() => {
  const players = selectedLivePlayers.value
  if (!players.length) {
    return null
  }

  return players.find((player) => player.id === activePlayerId.value) ?? players[0] ?? null
})

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
 * Whether a player selector should be shown.
 */
const showPlayerSelector = computed(() => selectedLivePlayers.value.length > 1)

/**
 * Resolves the currently selected live player into one media source consumable by the shared shell.
 *
 * @param player Embedded player selected for the current live.
 */
async function resolveSelectedLiveMedia(player: EntryPlayer | null) {
  const requestId = ++latestMediaResolutionRequestId

  resolvedLiveMediaSource.value = null
  resolvedLiveStream.value = null
  activeResolvedLiveStreamIndex.value = 0
  resolvedLiveMediaOpenUrl.value = player?.directLink ?? player?.webLink ?? null
  resolvedLiveMediaErrorMessage.value = null

  if (!selectedLiveItem.value?.source || !player) {
    return
  }

  isResolvedLiveMediaLoading.value = true

  try {
    const nextMediaSource = player.resolver
      ? await getStream(player).then((resolvedStream) =>
          !resolvedStream
            ? null
            : 'embedLink' in resolvedStream
              ? resolveIframeMediaSource(resolvedStream.embedLink)
              : (() => {
                  resolvedLiveStream.value = resolvedStream
                  return resolveBackendStreamMediaSource(
                    resolvedStream.streamUrl[0] ?? null,
                    resolvedStream.manifestType,
                    resolvedStream.licenseUrl,
                    resolvedStream.licenseHeaders,
                    resolvedStream.storyboardVttUrl,
                  )
                })(),
        )
      : resolvePlayerMediaSource(player.directLink)

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

/** Tries the next resolved live URL after a Video.js source error. */
function handleLiveMediaSourceError() {
  const stream = resolvedLiveStream.value
  const nextIndex = activeResolvedLiveStreamIndex.value + 1
  const nextUrl = stream?.streamUrl[nextIndex]
  if (!stream || !nextUrl) {
    resolvedLiveMediaErrorMessage.value = t('entry.videoUnavailable')
    return
  }

  activeResolvedLiveStreamIndex.value = nextIndex
  resolvedLiveMediaSource.value = resolveBackendStreamMediaSource(
    nextUrl,
    stream.manifestType,
    stream.licenseUrl,
    stream.licenseHeaders,
    stream.storyboardVttUrl,
  )
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

    // Restore initial live selection from route after list is loaded
    const source = props.initialSource?.trim()
    const channel = props.initialChannel?.trim()
    if (source && channel) {
      const matchingLiveItem = nextLiveItems.find(
        (item) => item.source === source && item.entryUrl === channel,
      )
      if (matchingLiveItem) {
        void selectLiveItem(matchingLiveItem, { shouldEmit: false })
      }
    }
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

   liveSelectionErrorMessage.value = null

   const cachedPlayers = livePlayersByItemId.value[liveItem.id]
   if (cachedPlayers && cachedPlayers.length > 0) {
     selectedLiveId.value = liveItem.id
     const cachedPlayer = cachedPlayers.find((player) => player.id === activePlayerId.value) ?? cachedPlayers[0] ?? null
     if (cachedPlayer) {
       activePlayerId.value = cachedPlayer.id
       await resolveSelectedLiveMedia(cachedPlayer)
     }
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

     selectedLiveId.value = liveItem.id

     const initialPlayer = players.find((player) => player.id === activePlayerId.value) ?? players[0] ?? null
     if (initialPlayer) {
       activePlayerId.value = initialPlayer.id
       await resolveSelectedLiveMedia(initialPlayer)
     }

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

/**
 * Handles the selected player update from the UI.
 */
function handleActivePlayerIdUpdate(value: string | null) {
  if (value === null || value === activePlayerId.value) {
    return
  }

  activePlayerId.value = value

  // Resolve the newly selected player immediately
  const player = selectedLivePlayers.value.find((p) => p.id === value) ?? null
  if (player) {
    void resolveSelectedLiveMedia(player)
  }
}

// Le watch d'initialisation a été supprimé volontairement. La restauration
// du live depuis la route est gérée directement dans loadLiveItems() après
// le chargement de la liste. Un watch séparé sur initialSource/initialChannel
// créerait un appel supplémentaire à getLivePlayers quand liveItems change
// (via JSON.stringify dans le watch), ce qui déclencherait un doublon.
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
    :show-trailer-action="false"
    :trailer-action-label="''"
    :entry-url="details?.entryUrl ?? null"
    :source="selectedLiveItem?.source ?? null"
    :alternative-title-label="details?.alternativeTitleLabel ?? null"
    :selected-playable-title="selectedPlayableItem?.title?.trim() ?? null"
    :show-bookmark-action="false"
    :show-trailer-player="false"
    :show-media-player="shouldShowMediaPlayer"
    :trailer-media-source="null"
    :media-source="resolvedLiveMediaSource"
    :media-open-url="resolvedLiveMediaOpenUrl"
    :is-media-player-loading="combinedMediaPlayerLoading"
    :media-player-error-message="combinedMediaPlayerErrorMessage"
    :show-player-controls="showPlayerSelector"
    :show-language-selector="false"
    :show-player-selector="showPlayerSelector"
    :available-languages="[]"
    :active-language-key="null"
    :filtered-players="selectedLivePlayers"
    :active-player-id="activePlayerId"
    :media-poster-url="mediaPosterUrl"
    :trailer-poster-url="null"
    :media-overlay-logo-url="null"
    :display-description="details?.description ?? t('live.prompt')"
    :is-full-width-content="!selectedLiveItem"
    :initial-playback-time="null"
    :media-autoplay="true"
    @source-error="handleLiveMediaSourceError"
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
     :score="null"
    @update:active-player-id="handleActivePlayerIdUpdate"
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
      item-route-name="live-details"
      @select-item="selectLiveItem"
    />
  </EntryDetails>
</template>
