import { computed, shallowRef, watch, type Ref } from 'vue'

import { t } from '@/i18n'
import {
  resolveBackendStreamMediaSource,
  resolveIframeMediaSource,
  resolvePlayerMediaSource,
  type ResolvedPlayerMediaSource,
  type ResolvedVideoChapter,
  type ResolvedVideoSpriteThumbnails,
} from '@/services/players'
import { getPlayers, getStream, normalizePlayersResponse } from '@/services/rustify'
import type { EntryDetails, EntryPlayableItem, EntryPlayer, EntryResolvedPlayerStream } from '@/types/entry'

/** Sentinel value used for players without a language code. */
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
  /** The stable language key used for selection. */
  key: string
  /** The human-readable language label displayed in the UI. */
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
  chapters: ResolvedVideoChapter[] | null = null,
): ResolvedPlayerMediaSource | null {
  if (!mediaSource || mediaSource.renderer !== 'video') {
    return mediaSource
  }

  return {
    ...mediaSource,
    storyboard,
    chapters: chapters ?? mediaSource.chapters,
  }
}

/**
 * Owns the complete built-in player state for one entry details view.
 *
 * @param options Reactive entry and playable-item sources used to keep the player synchronized.
 * @returns Player state, derived values, and explicit actions consumed by entry detail pages.
 */
export function entryVideoPlayer(options: UseEntryVideoPlayerOptions) {
  /** The currently selected language key in the language selector. */
  const selectedLanguageKey = shallowRef<string | null>(null)
  /** The currently selected player identifier. */
  const selectedPlayerId = shallowRef<string | null>(null)
  /** The language key selected for the current playable item. */
  const selectedPlayableLanguageKey = shallowRef<string | null>(null)
  /** The player identifier selected for the current playable item. */
  const selectedPlayablePlayerId = shallowRef<string | null>(null)
  /** The preferred language key persisted across playable item changes. */
  const preferredLanguageKey = shallowRef<string | null>(null)
  /** The preferred player key persisted across playable item changes. */
  const preferredPlayerKey = shallowRef<string | null>(null)
  /** The current video mode: 'media' for playable content or 'trailer' for trailer playback. */
  const activeVideoMode = shallowRef<'media' | 'trailer'>('media')
  /** The resolved media source for the current player, after backend resolution. */
  const resolvedMediaSource = shallowRef<ResolvedPlayerMediaSource | null>(null)
  /** Resolved video response retained to try its alternative URLs after a media error. */
  const resolvedStreamResponse = shallowRef<EntryResolvedPlayerStream | null>(null)
  /** Active URL index within the current resolved stream response. */
  const activeResolvedStreamIndex = shallowRef(0)
  /** Whether the media player is currently resolving a stream. */
  const isMediaPlayerLoading = shallowRef(false)
  /** Error message from media player resolution, or null if successful. */
  const mediaPlayerErrorMessage = shallowRef<string | null>(null)

  /** Request identifier counter for player resolution requests to ignore stale responses. */
  let activeResolutionId = 0

  /**
   * Exposes the backend source currently driving the active entry details view.
   *
   * @returns The source identifier or null.
   */
  const currentSource = computed(() => options.details.value?.source ?? null)

  /**
   * Exposes the optional trailer URL returned by the backend payload.
   *
   * @returns The trailer URL or null if not available.
   */
  const trailerUrl = computed(() => options.details.value?.trailerUrl ?? null)

  /**
   * Exposes the resolved trailer media source used by the embedded player.
   *
   * @returns The resolved media source for the trailer, or null.
   */
  const trailerMediaSource = computed<ResolvedPlayerMediaSource | null>(() =>
    resolvePlayerMediaSource(trailerUrl.value),
  )

  /**
   * Exposes the optional title image supplied by the currently resolved player.
   *
   * @returns Resolved player poster URL, or null when none is available.
   */
  const resolvedPlayerPosterUrl = computed(() => resolvedStreamResponse.value?.imageTitleLink ?? null)

  /**
   * Indicates whether the current entry exposes a trailer.
   *
   * @returns True if a trailer media source is available.
   */
  const hasTrailer = computed(() => Boolean(trailerMediaSource.value))

  /**
   * Exposes the embedded players returned by the backend entry payload.
   *
   * @returns Array of all players for the entry.
   */
  const availablePlayers = computed<EntryPlayer[]>(() => options.details.value?.players.entries ?? [])

  /**
   * Exposes the players that should drive the built-in player.
   * Uses playable item players if available, otherwise falls back to entry players.
   *
   * @returns Array of players relevant to the current playable context.
   */
  const activePlayers = computed<EntryPlayer[]>(() => {
    const selectedItemPlayers = options.selectedPlayableItem.value?.players.entries ?? []
    return selectedItemPlayers.length > 0 ? selectedItemPlayers : availablePlayers.value
  })


  const pendingPlayerLoads = new Map<string, Promise<EntryPlayer[]>>()

  async function loadDeferredPlayers(): Promise<void> {
    const item = options.selectedPlayableItem.value
    const collection = item?.players ?? options.details.value?.players

    if (!collection?.link || !collection.source || collection.entries.length > 0) {
      return
    }

    const key = collection.source + "|" + collection.link
    let request = pendingPlayerLoads.get(key)

    if (!request) {
      request = getPlayers(collection.source, collection.link).then((response) =>
        normalizePlayersResponse(response, collection.source),
      )
      pendingPlayerLoads.set(key, request)
    }

    isMediaPlayerLoading.value = true
    mediaPlayerErrorMessage.value = null

    try {
      const entries = await request
      const players = { ...collection, entries, link: undefined }

      if (item) {
        options.selectedPlayableItem.value = { ...item, players }
      } else if (options.details.value) {
        options.details.value = { ...options.details.value, players }
      }
    } catch (error) {
      mediaPlayerErrorMessage.value = t('entry.playerResolutionFailed')
    } finally {
      pendingPlayerLoads.delete(key)
      isMediaPlayerLoading.value = false
    }
  }
  /**
   * Exposes the language identifier currently selected in the UI.
   * Automatically switches between playable item and entry language selection.
   *
   * @returns The active language key.
   */
  const activeLanguageKey = computed<string | null>({
    get() {
      return options.selectedPlayableItem.value?.players.entries.length
        ? selectedPlayableLanguageKey.value
        : selectedLanguageKey.value
    },
    set(value) {
      if (options.selectedPlayableItem.value?.players.entries.length) {
        selectedPlayableLanguageKey.value = value
        return
      }

      selectedLanguageKey.value = value
    },
  })

  /**
   * Exposes the player identifier currently selected in the UI.
   * Automatically switches between playable item and entry player selection.
   *
   * @returns The active player identifier.
   */
  const activePlayerId = computed<string | null>({
    get() {
      return options.selectedPlayableItem.value?.players.entries.length
        ? selectedPlayablePlayerId.value
        : selectedPlayerId.value
    },
    set(value) {
      if (options.selectedPlayableItem.value?.players.entries.length) {
        selectedPlayablePlayerId.value = value
        return
      }

      selectedPlayerId.value = value
    },
  })

  /**
   * Exposes the distinct language filters available for the current player list.
   *
   * @returns Array of language options for the language selector.
   */
  const availableLanguages = computed(() => buildEntryPlayerLanguageOptions(activePlayers.value))

  /**
   * Exposes the effective language key driving the filtered player list.
   * Falls back to the first available language if the active one is not available.
   *
   * @returns The resolved language key or null.
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
   *
   * @returns Array of players filtered by the selected language.
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
   *
   * @returns The selected player, or the first filtered player, or null.
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
   *
   * @returns The playable item title or null.
   */
  const selectedPlayableTitle = computed(() =>
    options.selectedPlayableItem.value?.title?.trim() || null,
  )

  /**
   * Indicates whether the embedded language selector should be shown.
   *
   * @returns True when multiple languages are available.
   */
  const showLanguageSelector = computed(() => availableLanguages.value.length > 1)

  /**
   * Indicates whether the embedded player selector should be shown.
   *
   * @returns True when multiple players are available for the selected language.
   */
  const showPlayerSelector = computed(() => filteredPlayers.value.length > 1)

  /**
   * Indicates whether at least one selector should be rendered below the player.
   *
   * @returns True when language or player selector should be shown.
   */
  const showPlayerControls = computed(() =>
    showLanguageSelector.value || showPlayerSelector.value,
  )

  /**
   * Indicates whether the integrated player should prefer a backend-resolved stream.
   *
   * @returns True when the selected player has a resolver.
   */
  const shouldResolvePlayer = computed(() =>
    Boolean(selectedPlayer.value?.resolver),
  )

  /**
   * Indicates whether the selected player should be rendered through the iframe surface directly.
   *
   * @returns True when the player has an embed link and should use iframe mode.
   */
  /**
   * Exposes the direct media source that can be rendered without an extra backend request.
   * Handles direct links, iframe embeds, and storyboard attachment.
   *
   * @returns The resolved direct media source or null.
   */
  const directMediaSource = computed<ResolvedPlayerMediaSource | null>(() =>
    selectedPlayer.value?.directLink
      ? attachStoryboardToVideoSource(
          resolvePlayerMediaSource(selectedPlayer.value.directLink),
          resolvePlayerStoryboard(selectedPlayer.value),
        )
      : shouldResolvePlayer.value
          ? null
          : attachStoryboardToVideoSource(
              resolvePlayerMediaSource(null),
              resolvePlayerStoryboard(selectedPlayer.value),
            ),
  )

  /**
   * Exposes the media source currently selected for rendering.
   * Prefers resolved source when available.
   *
   * @returns The active media source for playback.
   */
  const mediaSource = computed<ResolvedPlayerMediaSource | null>(() =>
    shouldResolvePlayer.value
      ? resolvedMediaSource.value
      : directMediaSource.value ?? resolvedMediaSource.value,
  )

  /**
   * Exposes the public player URL that can still be opened externally when available.
   *
   * Prefers the player web link when present, then falls back to the direct link,
   * and finally to the resolver target.
   *
   * @returns The web, direct, or resolver target URL, or null.
   */
  const mediaOpenUrl = computed(() =>
    selectedPlayer.value?.webLink ??
    selectedPlayer.value?.directLink ??
    null
  )

  /**
   * Indicates whether the current player can eventually produce media for the built-in surface.
   *
   * @returns True when a direct link, iframe embed, or resolver is available.
   */
  const hasMediaCandidate = computed(() =>
    selectedPlayer.value?.directLink
      ? true
      : shouldResolvePlayer.value
          ? Boolean(selectedPlayer.value?.resolver)
          : Boolean(directMediaSource.value || selectedPlayer.value?.resolver),
  )

  /**
   * Keeps the media surface mounted when one playable selection is available but no trailer
   * can cover the transition state.
   *
   * @returns True when players are available and a playable item is selected or no trailer exists.
   */
  const shouldKeepMediaSurfaceVisible = computed(() =>
    activePlayers.value.length > 0 &&
    (Boolean(options.selectedPlayableItem.value) || !hasTrailer.value),
  )

  /**
   * Exposes whether the built-in trailer player should be rendered.
   *
   * @returns True when a trailer is available and should be shown.
   */
  const showTrailerPlayer = computed(() =>
    hasTrailer.value &&
    (activeVideoMode.value === 'trailer' || !mediaSource.value),
  )

  /**
   * Exposes whether the built-in media player should be rendered.
   *
   * @returns True when in media mode and a candidate, loading state, or error exists.
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
   *
   * @returns True when a trailer is available.
   */
  const showTrailerAction = computed(() => hasTrailer.value)

  /**
   * Exposes the trailer action label depending on the current player mode.
   *
   * @returns The label for the trailer toggle action.
   */
  const trailerActionLabel = computed(() =>
    activeVideoMode.value === 'trailer' && hasMediaCandidate.value ? t('entry.video'):  t('entry.trailer'),
  )

  /**
   * Clears the selected-item-specific language and player state.
   */
  function clearSelectedPlayablePlayerState(): void {
    selectedPlayableLanguageKey.value = null
    selectedPlayablePlayerId.value = null
  }

  /**
   * Clears the current player state before loading another entry.
   */
  function resetForEntryLoad(): void {
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
   * @param nextDetails - Detailed entry returned by the backend.
   */
  function applyLoadedEntryDefaults(nextDetails: EntryDetails): void {
    activeVideoMode.value = nextDetails.players.entries.length > 0
      ? 'media'
      : (resolvePlayerMediaSource(nextDetails.trailerUrl) ? 'trailer' : 'media')
    selectedLanguageKey.value = preferredLanguageKey.value
    selectedPlayerId.value = null
    clearSelectedPlayablePlayerState()
  }

  /**
   * Toggles the built-in player between the trailer and the current media video.
   */
  function handleTrailerToggle(): void {
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
  function activateMediaPlayer(): void {
    activeVideoMode.value = 'media'
    void loadDeferredPlayers()
  }

  /**
   * Stores the currently selected language as the preferred language for the next playable video.
   */
  function rememberCurrentLanguage(): void {
    preferredLanguageKey.value = resolvedLanguageKey.value
  }

  /**
   * Stores the currently selected player as the preferred player for the next playable video.
   */
  function rememberCurrentPlayer(): void {
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
      resolvedStreamResponse.value = null
      activeResolvedStreamIndex.value = 0
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
        const resolvedStream = await getStream(currentPlayer)

        if (resolutionId !== activeResolutionId) {
          return
        }

        const nextMediaSource = !resolvedStream
          ? null
          : 'embedLink' in resolvedStream
            ? resolveIframeMediaSource(resolvedStream.embedLink)
            : (() => {
                resolvedStreamResponse.value = resolvedStream
                return resolveBackendStreamMediaSource(
                  resolvedStream.streamUrl[0] ?? null,
                  resolvedStream.manifestType,
                  resolvedStream.licenseUrl,
                  resolvedStream.licenseHeaders,
                  resolvedStream.storyboardVttUrl,
                  resolvedStream.chapters ?? undefined,
                )
              })()

        if (!nextMediaSource) {
          mediaPlayerErrorMessage.value = t('entry.videoUnavailable')
          return
        }

        // Prefer storyboard from the resolved stream (returned by resolver),
        // fall back to the player-carried storyboard for backward compatibility.
        const resolvedStoryboard = !resolvedStream || 'embedLink' in resolvedStream
          ? null
          : (resolvedStream.storyboard ?? resolvePlayerStoryboard(currentPlayer))

        resolvedMediaSource.value = attachStoryboardToVideoSource(
          nextMediaSource,
          resolvedStoryboard,
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

  /** Tries the next URL returned by the current resolver after a Video.js source failure. */
  function handleMediaSourceError(): void {
    const stream = resolvedStreamResponse.value
    if (!stream) {
      mediaPlayerErrorMessage.value = t('entry.videoUnavailable')
      return
    }

    const nextIndex = activeResolvedStreamIndex.value + 1
    const nextUrl = stream.streamUrl[nextIndex]
    if (!nextUrl) {
      mediaPlayerErrorMessage.value = t('entry.videoUnavailable')
      return
    }

    activeResolvedStreamIndex.value = nextIndex
    resolvedMediaSource.value = attachStoryboardToVideoSource(
      resolveBackendStreamMediaSource(
        nextUrl,
        stream.manifestType,
        stream.licenseUrl,
        stream.licenseHeaders,
        stream.storyboardVttUrl,
        stream.chapters ?? undefined,
      ),
      stream.storyboard ?? resolvePlayerStoryboard(selectedPlayer.value),
      stream.chapters ?? null,
    )
  }

  watch(
    () => options.selectedPlayableItem.value?.id ?? null,
    (nextPlayableItemId, previousPlayableItemId) => {
      if (!nextPlayableItemId) {
        clearSelectedPlayablePlayerState()
        return
      }

      if (previousPlayableItemId !== nextPlayableItemId) {
        const selectedItemPlayers = options.selectedPlayableItem.value?.players.entries ?? []
        const nextLanguageOptions = buildEntryPlayerLanguageOptions(selectedItemPlayers)
        const preferredPlayerInAnyLanguage = preferredPlayerKey.value
          ? selectedItemPlayers.find(
              (player) => getPlayerPreferenceKey(player) === preferredPlayerKey.value,
            )
          : null
        const nextLanguageKey =
          preferredLanguageKey.value &&
          nextLanguageOptions.some((language) => language.key === preferredLanguageKey.value)
            ? preferredLanguageKey.value
            : (preferredPlayerInAnyLanguage
                ? getPlayerLanguageKey(preferredPlayerInAnyLanguage)
                : nextLanguageOptions[0]?.key ?? null)
        const playersInSelectedLanguage = selectedItemPlayers.filter(
          (player) => getPlayerLanguageKey(player) === nextLanguageKey,
        )
        const preferredPlayer = preferredPlayerKey.value
          ? playersInSelectedLanguage.find(
              (player) => getPlayerPreferenceKey(player) === preferredPlayerKey.value,
            )
          : null

        selectedPlayableLanguageKey.value = nextLanguageKey
        selectedPlayablePlayerId.value = preferredPlayer?.id ?? playersInSelectedLanguage[0]?.id ?? null
        activeVideoMode.value = 'media'
      }
    },
  )

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
    resolvedPlayerPosterUrl,
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
    handleMediaSourceError,
  }
}
