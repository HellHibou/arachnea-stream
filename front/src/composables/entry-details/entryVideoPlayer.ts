import { computed, shallowRef, watch, type Ref } from 'vue'

import { t } from '@/i18n'
import {
  resolveBackendStreamMediaSource,
  resolveIframeMediaSource,
  resolvePlayerMediaSource,
  type ResolvedPlayerMediaSource,
  type ResolvedVideoSpriteThumbnails,
} from '@/services/players'
import { resolvePlayerStream } from '@/services/rustify'
import type { EntryDetails, EntryPlayableItem, EntryPlayer } from '@/types/entry'

const unknownLanguageKey = '__unknown__'

/**
 * Options accepted by the entry video player controller.
 */
interface UseEntryVideoPlayerOptions {
  /**
   * Loaded entry details driving the active player state.
   */
  details: Ref<EntryDetails | null>
  /**
   * Playable item currently selected in the built-in player.
   */
  selectedPlayableItem: Ref<EntryPlayableItem | null>
}

/**
 * Display-ready language option rendered in the embedded player selector.
 */
export interface EntryPlayerLanguageOption {
  key: string
  label: string
}

/**
 * Returns a stable grouping key for one player language.
 *
 * @param player Player rendered by the current built-in player context.
 * @returns Stable language key used by the language selector.
 */
function getPlayerLanguageKey(player: EntryPlayer): string {
  return player.lang?.trim().toLocaleLowerCase() || unknownLanguageKey
}

/**
 * Formats one player language for display in the language selector.
 *
 * @param player Player rendered by the current built-in player context.
 * @returns Human-readable language label.
 */
function formatPlayerLanguageLabel(player: EntryPlayer): string {
  return player.lang?.trim().toLocaleUpperCase() || t('player.undefined')
}

/**
 * Returns a stable preference key for one player across item changes.
 *
 * @param player Player rendered by the current built-in player context.
 * @returns Stable player key used to restore the last selected player when available.
 */
function getPlayerPreferenceKey(player: EntryPlayer): string {
  return player.name?.trim().toLocaleLowerCase() || player.label.trim().toLocaleLowerCase()
}

/**
 * Returns embedded players in backend order.
 *
 * The backend order is used as the default priority when several external
 * iframe players are available for the same playable item or entry.
 *
 * @param players Players rendered by the current built-in player context.
 * @returns New array preserving backend order.
 */
function sortEntryPlayers(players: EntryPlayer[]): EntryPlayer[] {
  return [...players]
}

/**
 * Builds the language selector options from the current player list.
 *
 * @param players Players rendered by the current built-in player context.
 * @returns Deduplicated language options preserving backend order.
 */
function buildEntryPlayerLanguageOptions(
  players: EntryPlayer[],
): EntryPlayerLanguageOption[] {
  const options: EntryPlayerLanguageOption[] = []
  const seen = new Set<string>()

  players.forEach((player) => {
    const key = getPlayerLanguageKey(player)

    if (seen.has(key)) {
      return
    }

    seen.add(key)
    options.push({
      key,
      label: formatPlayerLanguageLabel(player),
    })
  })

  return options
}

/**
 * Copies one normalized storyboard into the media-source format consumed by Video.js.
 *
 * @param player Player currently selected in the UI.
 * @returns Sprite thumbnail configuration when available.
 */
function resolvePlayerStoryboard(player: EntryPlayer | null): ResolvedVideoSpriteThumbnails | null {
  if (!player?.storyboard) {
    return null
  }

  return { ...player.storyboard }
}

/**
 * Enriches one resolved media source with storyboard metadata when it renders through Video.js.
 *
 * @param mediaSource Resolved player media source.
 * @param storyboard Sprite thumbnail configuration carried by the selected player.
 * @returns Video source including storyboard metadata when applicable.
 */
function attachStoryboardToVideoSource(
  mediaSource: ResolvedPlayerMediaSource | null,
  storyboard: ResolvedVideoSpriteThumbnails | null,
): ResolvedPlayerMediaSource | null {
  if (!mediaSource || mediaSource.renderer !== 'video') {
    return mediaSource
  }

  return {
    ...mediaSource,
    storyboard,
  }
}

/**
 * Owns the complete built-in player state for one entry details view.
 *
 * @param options Reactive entry and playable-item sources used to keep the player synchronized.
 * @returns Player state, derived values, and explicit actions consumed by entry detail pages.
 */
export function entryVideoPlayer(options: UseEntryVideoPlayerOptions) {
  const selectedLanguageKey = shallowRef<string | null>(null)
  const selectedPlayerId = shallowRef<string | null>(null)
  const selectedPlayableLanguageKey = shallowRef<string | null>(null)
  const selectedPlayablePlayerId = shallowRef<string | null>(null)
  const preferredLanguageKey = shallowRef<string | null>(null)
  const preferredPlayerKey = shallowRef<string | null>(null)
  const activeVideoMode = shallowRef<'media' | 'trailer'>('media')
  const resolvedMediaSource = shallowRef<ResolvedPlayerMediaSource | null>(null)
  const isMediaPlayerLoading = shallowRef(false)
  const mediaPlayerErrorMessage = shallowRef<string | null>(null)

  let activeResolutionId = 0

  /**
   * Exposes the backend source currently driving the active entry details view.
   */
  const currentSource = computed(() => options.details.value?.source ?? null)

  /**
   * Exposes the optional trailer URL returned by the backend payload.
   */
  const trailerUrl = computed(() => options.details.value?.trailerUrl ?? null)

  /**
   * Exposes the resolved trailer media source used by the embedded player.
   */
  const trailerMediaSource = computed<ResolvedPlayerMediaSource | null>(() =>
    resolvePlayerMediaSource(trailerUrl.value),
  )

  /**
   * Indicates whether the current entry exposes a trailer.
   */
  const hasTrailer = computed(() => Boolean(trailerMediaSource.value))

  /**
   * Exposes the embedded players returned by the backend entry payload.
   */
  const availablePlayers = computed<EntryPlayer[]>(() => options.details.value?.players ?? [])

  /**
   * Exposes the players that should drive the built-in player.
   */
  const activePlayers = computed<EntryPlayer[]>(() => {
    const selectedItemPlayers = options.selectedPlayableItem.value?.players ?? []
    return selectedItemPlayers.length > 0 ? selectedItemPlayers : availablePlayers.value
  })

  /**
   * Exposes the language identifier currently selected in the UI.
   */
  const activeLanguageKey = computed<string | null>({
    get() {
      return options.selectedPlayableItem.value?.players.length
        ? selectedPlayableLanguageKey.value
        : selectedLanguageKey.value
    },
    set(value) {
      if (options.selectedPlayableItem.value?.players.length) {
        selectedPlayableLanguageKey.value = value
        return
      }

      selectedLanguageKey.value = value
    },
  })

  /**
   * Exposes the player identifier currently selected in the UI.
   */
  const activePlayerId = computed<string | null>({
    get() {
      return options.selectedPlayableItem.value?.players.length
        ? selectedPlayablePlayerId.value
        : selectedPlayerId.value
    },
    set(value) {
      if (options.selectedPlayableItem.value?.players.length) {
        selectedPlayablePlayerId.value = value
        return
      }

      selectedPlayerId.value = value
    },
  })

  /**
   * Exposes the distinct language filters available for the current player list.
   */
  const availableLanguages = computed(() => buildEntryPlayerLanguageOptions(activePlayers.value))

  /**
   * Exposes the effective language key driving the filtered player list.
   */
  const resolvedLanguageKey = computed<string | null>(() => {
    if (availableLanguages.value.length === 0) {
      return null
    }

    if (
      activeLanguageKey.value &&
      availableLanguages.value.some((language) => language.key === activeLanguageKey.value)
    ) {
      return activeLanguageKey.value
    }

    return availableLanguages.value[0]?.key ?? null
  })

  /**
   * Exposes the players available for the selected language.
   */
  const filteredPlayers = computed<EntryPlayer[]>(() => {
    if (availableLanguages.value.length < 2) {
      return sortEntryPlayers(activePlayers.value)
    }

    const currentLanguageKey = resolvedLanguageKey.value
    return sortEntryPlayers(
      activePlayers.value.filter((player) => getPlayerLanguageKey(player) === currentLanguageKey),
    )
  })

  /**
   * Resolves the embedded player currently selected in the UI.
   */
  const selectedPlayer = computed<EntryPlayer | null>(() => {
    const players = filteredPlayers.value

    if (!players.length) {
      return null
    }

    return players.find((player) => player.id === activePlayerId.value) ?? players[0] ?? null
  })

  /**
   * Exposes the selected playable item title displayed above the built-in player.
   */
  const selectedPlayableTitle = computed(() =>
    options.selectedPlayableItem.value?.title?.trim() || null,
  )

  /**
   * Indicates whether the embedded language selector should be shown.
   */
  const showLanguageSelector = computed(() => availableLanguages.value.length > 1)

  /**
   * Indicates whether the embedded player selector should be shown.
   */
  const showPlayerSelector = computed(() => filteredPlayers.value.length > 1)

  /**
   * Indicates whether at least one selector should be rendered below the player.
   */
  const showPlayerControls = computed(() =>
    showLanguageSelector.value || showPlayerSelector.value,
  )

  /**
   * Indicates whether the integrated player should prefer a backend-resolved stream.
   */
  const shouldResolvePlayer = computed(() =>
    Boolean(selectedPlayer.value?.resolver),
  )

  /**
   * Indicates whether the selected player should be rendered through the iframe surface directly.
   */
  const shouldForceIframePlayer = computed(() =>
    Boolean(selectedPlayer.value?.embedLink) &&
    (
      activePlayers.value.length > 1 ||
      (Boolean(options.selectedPlayableItem.value) && !hasTrailer.value)
    ),
  )

  /**
   * Exposes the direct media source that can be rendered without an extra backend request.
   */
  const directMediaSource = computed<ResolvedPlayerMediaSource | null>(() =>
    selectedPlayer.value?.directLink
      ? attachStoryboardToVideoSource(
          resolvePlayerMediaSource(selectedPlayer.value.directLink),
          resolvePlayerStoryboard(selectedPlayer.value),
        )
      : shouldForceIframePlayer.value
        ? resolveIframeMediaSource(selectedPlayer.value?.embedLink ?? null)
        : shouldResolvePlayer.value
          ? null
          : attachStoryboardToVideoSource(
              resolvePlayerMediaSource(selectedPlayer.value?.embedLink ?? null),
              resolvePlayerStoryboard(selectedPlayer.value),
            ),
  )

  /**
   * Exposes the media source currently selected for rendering.
   */
  const mediaSource = computed<ResolvedPlayerMediaSource | null>(() =>
    shouldResolvePlayer.value
      ? resolvedMediaSource.value
      : directMediaSource.value ?? resolvedMediaSource.value,
  )

  /**
   * Exposes the public player URL that can still be opened externally when available.
   */
  const mediaOpenUrl = computed(() =>
    selectedPlayer.value?.directLink ?? selectedPlayer.value?.embedLink ?? null
  )

  /**
   * Indicates whether the current player can eventually produce media for the built-in surface.
   */
  const hasMediaCandidate = computed(() =>
    selectedPlayer.value?.directLink
      ? true
      : shouldForceIframePlayer.value
        ? Boolean(selectedPlayer.value?.embedLink)
        : shouldResolvePlayer.value
          ? Boolean(selectedPlayer.value?.resolver)
          : Boolean(directMediaSource.value || selectedPlayer.value?.resolver),
  )

  /**
   * Keeps the media surface mounted when one playable selection is available but no trailer
   * can cover the transition state.
   */
  const shouldKeepMediaSurfaceVisible = computed(() =>
    activePlayers.value.length > 0 &&
    (Boolean(options.selectedPlayableItem.value) || !hasTrailer.value),
  )

  /**
   * Exposes whether the built-in trailer player should be rendered.
   */
  const showTrailerPlayer = computed(() =>
    hasTrailer.value &&
    (activeVideoMode.value === 'trailer' || !mediaSource.value),
  )

  /**
   * Exposes whether the built-in media player should be rendered.
   */
  const showMediaPlayer = computed(() =>
    activeVideoMode.value !== 'trailer' &&
    (
      hasMediaCandidate.value ||
      shouldKeepMediaSurfaceVisible.value ||
      isMediaPlayerLoading.value ||
      Boolean(mediaPlayerErrorMessage.value)
    ),
  )

  /**
   * Exposes whether the trailer toggle action should be shown below the cover.
   */
  const showTrailerAction = computed(() => hasTrailer.value)

  /**
   * Exposes the trailer action label depending on the current player mode.
   */
  const trailerActionLabel = computed(() =>
    activeVideoMode.value === 'trailer' && hasMediaCandidate.value ? t('entry.video'):  t('entry.trailer'),
  )

  /**
   * Clears the selected-item-specific language and player state.
   */
  function clearSelectedPlayablePlayerState() {
    selectedPlayableLanguageKey.value = null
    selectedPlayablePlayerId.value = null
  }

  /**
   * Clears the current player state before loading another entry.
   */
  function resetForEntryLoad() {
    activeResolutionId += 1
    activeVideoMode.value = 'media'
    selectedLanguageKey.value = null
    selectedPlayerId.value = null
    clearSelectedPlayablePlayerState()
    resolvedMediaSource.value = null
    isMediaPlayerLoading.value = false
    mediaPlayerErrorMessage.value = null
  }

  /**
   * Applies the initial player state for one freshly loaded entry.
   *
   * @param nextDetails Detailed entry returned by the backend.
   */
  function applyLoadedEntryDefaults(nextDetails: EntryDetails) {
    activeVideoMode.value = nextDetails.players.length > 0
      ? 'media'
      : (resolvePlayerMediaSource(nextDetails.trailerUrl) ? 'trailer' : 'media')
    selectedLanguageKey.value = preferredLanguageKey.value
    selectedPlayerId.value = null
    clearSelectedPlayablePlayerState()
  }

  /**
   * Toggles the built-in player between the trailer and the current media video.
   */
  function handleTrailerToggle() {
    if (!hasTrailer.value) {
      return
    }

    if (hasMediaCandidate.value) {
      activeVideoMode.value = activeVideoMode.value === 'trailer' ? 'media' : 'trailer'
      return
    }

    activeVideoMode.value = 'trailer'
  }

  /**
   * Forces the built-in player back to media mode after an explicit selection.
   */
  function activateMediaPlayer() {
    activeVideoMode.value = 'media'
  }

  /**
   * Stores the currently selected language as the preferred language for the next playable video.
   */
  function rememberCurrentLanguage() {
    preferredLanguageKey.value = resolvedLanguageKey.value
  }

  /**
   * Stores the currently selected player as the preferred player for the next playable video.
   */
  function rememberCurrentPlayer() {
    if (!selectedPlayer.value) {
      return
    }

    preferredPlayerKey.value = getPlayerPreferenceKey(selectedPlayer.value)
  }

  watch(options.details, (nextDetails) => {
    if (!nextDetails) {
      resetForEntryLoad()
      return
    }

    applyLoadedEntryDefaults(nextDetails)
  }, { immediate: true })

  watch(availableLanguages, (languages) => {
    if (languages.length === 0) {
      activeLanguageKey.value = null
      return
    }

    if (languages.some((language) => language.key === activeLanguageKey.value)) {
      return
    }

    if (
      preferredLanguageKey.value &&
      languages.some((language) => language.key === preferredLanguageKey.value)
    ) {
      activeLanguageKey.value = preferredLanguageKey.value
      return
    }

    activeLanguageKey.value = languages[0]?.key ?? null
  }, { immediate: true })

  watch(filteredPlayers, (players) => {
    if (players.length === 0) {
      activePlayerId.value = null
      return
    }

    if (players.some((player) => player.id === activePlayerId.value)) {
      return
    }

    const preferredPlayer = preferredPlayerKey.value
      ? players.find((player) => getPlayerPreferenceKey(player) === preferredPlayerKey.value)
      : null

    activePlayerId.value = preferredPlayer?.id ?? players[0]?.id ?? null
  }, { immediate: true })

  watch(
    [currentSource, () => selectedPlayer.value?.id ?? null],
    async ([source, nextSelectedPlayerId]) => {
      activeResolutionId += 1
      const resolutionId = activeResolutionId

      resolvedMediaSource.value = null
      mediaPlayerErrorMessage.value = null
      isMediaPlayerLoading.value = false

      if (!nextSelectedPlayerId || !shouldResolvePlayer.value) {
        return
      }

      const currentPlayer = selectedPlayer.value
      if (!source || !currentPlayer?.resolver) {
        return
      }

      isMediaPlayerLoading.value = true

      try {
        const resolvedStream = await resolvePlayerStream(source, currentPlayer)

        if (resolutionId !== activeResolutionId) {
          return
        }

        const nextMediaSource = resolvedStream
          ? resolveBackendStreamMediaSource(
              resolvedStream.streamUrl,
              resolvedStream.manifestType,
              resolvedStream.licenseUrl,
              resolvedStream.licenseHeaders,
            )
          : null

        if (!nextMediaSource) {
          mediaPlayerErrorMessage.value = t('entry.videoUnavailable')
          return
        }

        resolvedMediaSource.value = attachStoryboardToVideoSource(
          nextMediaSource,
          resolvePlayerStoryboard(currentPlayer),
        )
      } catch (error) {
        if (resolutionId !== activeResolutionId) {
          return
        }

        mediaPlayerErrorMessage.value =
          error instanceof Error
            ? error.message
            : t('errors.livePlayer')
      } finally {
        if (resolutionId === activeResolutionId) {
          isMediaPlayerLoading.value = false
        }
      }
    },
    { immediate: true },
  )

  watch(
    () => options.selectedPlayableItem.value?.id ?? null,
    (nextPlayableItemId, previousPlayableItemId) => {
      if (!nextPlayableItemId) {
        clearSelectedPlayablePlayerState()
        return
      }

      if (previousPlayableItemId !== nextPlayableItemId) {
        selectedPlayableLanguageKey.value = preferredLanguageKey.value
        selectedPlayablePlayerId.value = null
        activeVideoMode.value = 'media'
      }
    },
  )

  watch(options.selectedPlayableItem, (selectedPlayableItem) => {
    if (!selectedPlayableItem || selectedPlayableItem.players.length === 0) {
      return
    }

    if (
      selectedPlayableItem.players.some(
        (player) => player.id === selectedPlayablePlayerId.value,
      )
    ) {
      return
    }

    selectedPlayableLanguageKey.value = preferredLanguageKey.value
    selectedPlayablePlayerId.value = null
  })

  watch(activeLanguageKey, (nextLanguageKey, previousLanguageKey) => {
    if (
      previousLanguageKey !== undefined &&
      nextLanguageKey !== previousLanguageKey &&
      hasMediaCandidate.value
    ) {
      activeVideoMode.value = 'media'
    }
  })

  watch(activePlayerId, (nextPlayerId, previousPlayerId) => {
    if (
      previousPlayerId !== undefined &&
      nextPlayerId !== previousPlayerId &&
      hasMediaCandidate.value
    ) {
      activeVideoMode.value = 'media'
    }
  })

  watch(mediaSource, (nextMediaSource) => {
    if (nextMediaSource && options.selectedPlayableItem.value) {
      activeVideoMode.value = 'media'
      return
    }

    if (!nextMediaSource && !hasMediaCandidate.value && hasTrailer.value) {
      activeVideoMode.value = 'trailer'
    }
  })

  return {
    activeLanguageKey,
    availableLanguages,
    filteredPlayers,
    activePlayerId,
    mediaSource,
    mediaOpenUrl,
    isMediaPlayerLoading,
    mediaPlayerErrorMessage,
    trailerUrl,
    trailerMediaSource,
    showTrailerPlayer,
    selectedPlayableTitle,
    showMediaPlayer,
    showTrailerAction,
    trailerActionLabel,
    showLanguageSelector,
    showPlayerSelector,
    showPlayerControls,
    activateMediaPlayer,
    handleTrailerToggle,
    rememberCurrentLanguage,
    rememberCurrentPlayer,
  }
}
