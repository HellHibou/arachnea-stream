import { computed, shallowRef, watch, type Ref } from 'vue'
import { t } from '@/i18n'
import { resolveBackendStreamMediaSource, resolveIframeMediaSource, resolvePlayerMediaSource, type ResolvedPlayerMediaSource } from '@/services/players'
import { getPlayers, getStream, normalizePlayersResponse } from '@/services/rustify'
import type { EntryPlayer, EntryResolvedPlayerStream } from '@/types/entry'
import type { UseEntryVideoPlayerOptions } from './entryPlayerTypes'
import { attachStoryboardToVideoSource, resolvePlayerStoryboard, resolvePlayerChapters, resolvePlayerSubtitles } from './entryPlayerMedia'

interface EntryPlayerStreamOptions extends Pick<UseEntryVideoPlayerOptions, 'details' | 'selectedPlayableItem'> {
  selectedPlayer: Readonly<Ref<EntryPlayer | null>>
}

/**
 * Owns deferred players, stream resolution, loading/error state and alternate URLs.
 * @param options Reactive entry/item refs and the current selected player.
 * @returns Media state, source-failure handling and explicit lifecycle operations.
 */
export function createEntryPlayerStream(options: EntryPlayerStreamOptions) {
  const { selectedPlayer } = options
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
   * Exposes the optional title image supplied by the currently resolved player.
   *
   * @returns Resolved player poster URL, or null when none is available.
   */
  const resolvedPlayerPosterUrl = computed(() => resolvedStreamResponse.value?.imageTitleLink ?? null)

  const pendingPlayerLoads = new Map<string, Promise<EntryPlayer[]>>()

  /** Loads player descriptors once per source/link while sharing resolution loading state. */
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
   * Indicates whether the integrated player should prefer a backend-resolved stream.
   *
   * @returns True when the selected player has a resolver.
   */
  const shouldResolvePlayer = computed(() =>
    Boolean(selectedPlayer.value?.resolver),
  )

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
        resolvePlayerChapters(selectedPlayer.value),
        resolvePlayerSubtitles(selectedPlayer.value),
      )
      : shouldResolvePlayer.value
        ? null
        : attachStoryboardToVideoSource(
          resolvePlayerMediaSource(null),
          resolvePlayerStoryboard(selectedPlayer.value),
          resolvePlayerChapters(selectedPlayer.value),
          resolvePlayerSubtitles(selectedPlayer.value),
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
   * Prefers the player web link when present, then falls back to the direct link.
   *
   * @returns The web or direct URL, or null.
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
      resolveBackendStreamMediaSource({
        streamUrl: nextUrl,
        manifestType: stream.manifestType,
        licenseUrl: stream.licenseUrl,
        licenseHeaders: stream.licenseHeaders,
        storyboardVttUrl: stream.storyboardVttUrl,
        chapters: stream.chapters,
        subtitles: stream.subtitles,
      }),
      stream.storyboard ?? resolvePlayerStoryboard(selectedPlayer.value),
      stream.chapters ?? null,
      stream.subtitles.length > 0
        ? stream.subtitles
        : resolvePlayerSubtitles(selectedPlayer.value),
    )
  }

  /** Invalidates active stream resolution and clears entry-scoped media state. */
  function resetForEntryLoad() {
    activeResolutionId += 1
    resolvedMediaSource.value = null
    isMediaPlayerLoading.value = false
    mediaPlayerErrorMessage.value = null
  }

  /** Installs the source/player resolver watcher after trailer observation. */
  function observe() {
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
                return resolveBackendStreamMediaSource({
                  streamUrl: resolvedStream.streamUrl[0] ?? null,
                  manifestType: resolvedStream.manifestType,
                  licenseUrl: resolvedStream.licenseUrl,
                  licenseHeaders: resolvedStream.licenseHeaders,
                  storyboardVttUrl: resolvedStream.storyboardVttUrl,
                  chapters: resolvedStream.chapters,
                  subtitles: resolvedStream.subtitles,
                })
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
          const resolvedSubtitles = !resolvedStream || 'embedLink' in resolvedStream
            ? null
            : (resolvedStream.subtitles.length > 0
              ? resolvedStream.subtitles
              : resolvePlayerSubtitles(currentPlayer))

          resolvedMediaSource.value = attachStoryboardToVideoSource(
            nextMediaSource,
            resolvedStoryboard,
            null,
            resolvedSubtitles,
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
  }

  return {
    mediaSource, mediaOpenUrl, hasMediaCandidate, resolvedPlayerPosterUrl,
    isMediaPlayerLoading, mediaPlayerErrorMessage, loadDeferredPlayers,
    handleMediaSourceError, resetForEntryLoad, observe,
  }
}
