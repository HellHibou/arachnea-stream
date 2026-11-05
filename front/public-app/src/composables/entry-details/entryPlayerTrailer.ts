import { computed, shallowRef, watch, type Ref } from 'vue'
import { t } from '@/i18n'
import { resolveBackendStreamMediaSource, resolveIframeMediaSource, resolvePlayerMediaSource, type ResolvedPlayerMediaSource } from '@/services/players'
import { getStream } from '@/services/rustify'
import { isMediaSourceAllowedBySecurity } from '@/services/videoSourceAllowlist'
import type { EntryPlayer } from '@/types/entry'
import type { UseEntryVideoPlayerOptions } from './entryPlayerTypes'

interface EntryPlayerTrailerOptions extends Pick<UseEntryVideoPlayerOptions, 'details' | 'securityMode' | 'useTrailerAsBackground'> {
  activeVideoMode: Readonly<Ref<'media' | 'trailer'>>
}

/**
 * Owns trailer resolution, its stale-response counter and security availability.
 * @param options Reactive entry/security preferences and current surface mode.
 * @returns Trailer state and explicit reset/observer operations.
 */
export function createEntryPlayerTrailer(options: EntryPlayerTrailerOptions) {
  const { activeVideoMode } = options
  /** Request identifier counter for trailer resolution requests to ignore stale responses. */
  let activeTrailerResolutionId = 0

  /**
   * Exposes the optional trailer URL returned by the backend payload.
   *
   * @returns The trailer URL or null if not available.
   */
  const trailerUrl = computed(() => options.details.value?.trailerUrl ?? null)

  /** Trailer resolver returned separately from direct `video/trailer` values. */
  const trailerResolver = computed(() => options.details.value?.trailerResolver ?? null)

  /** Resolved source for a trailer with an ephemeral stream URL. */
  const resolvedTrailerMediaSource = shallowRef<ResolvedPlayerMediaSource | null>(null)

  /**
   * Exposes the resolved trailer media source used by the embedded player.
   *
   * @returns The resolved media source for the trailer, or null.
   */
  const trailerMediaSource = computed<ResolvedPlayerMediaSource | null>(() =>
    resolvedTrailerMediaSource.value ?? resolvePlayerMediaSource(trailerUrl.value),
  )

  /**
   * Indicates whether the current entry exposes a trailer.
   *
   * @returns True if a trailer media source is available.
   */
  const hasTrailer = computed(() => Boolean(trailerMediaSource.value || trailerResolver.value))

  /**
   * Indicates whether the trailer iframe is blocked without a confirmation path.
   *
   * @returns True for a non-allowlisted trailer iframe in `safe` mode.
   */
  const isTrailerBlocked = computed(() =>
    options.securityMode.value === 'safe' &&
    trailerMediaSource.value?.renderer === 'iframe' &&
    !isMediaSourceAllowedBySecurity(
      trailerMediaSource.value,
      options.securityMode.value,
    ),
  )

  /** Invalidates active trailer resolution and clears the previous trailer source. */
  function resetForEntryLoad() {
    activeTrailerResolutionId += 1
    resolvedTrailerMediaSource.value = null
  }

  /** Installs trailer resolution before the primary stream resolver watcher. */
  function observe() {
    watch(
      [trailerResolver, () => activeVideoMode.value, () => options.useTrailerAsBackground.value],
      async ([resolver, videoMode, useTrailerAsBackground]) => {
        if (
          !resolver ||
          (videoMode !== 'trailer' && !useTrailerAsBackground) ||
          resolvedTrailerMediaSource.value
        ) {
          return
        }

        const resolutionId = ++activeTrailerResolutionId
        const trailerPlayer: EntryPlayer = {
          id: `trailer:${resolver.kind}:${resolver.targetId}`,
          label: t('entry.trailer'),
          directLink: null,
          webLink: null,
          name: null,
          lang: null,
          resolver,
          storyboard: null,
          chapters: null,
          subtitles: [],
        }

        try {
          const resolvedStream = await getStream(trailerPlayer)
          if (resolutionId !== activeTrailerResolutionId || !resolvedStream) {
            return
          }
          resolvedTrailerMediaSource.value = 'embedLink' in resolvedStream
            ? resolveIframeMediaSource(resolvedStream.embedLink)
            : resolveBackendStreamMediaSource({
              streamUrl: resolvedStream.streamUrl[0] ?? null,
              manifestType: resolvedStream.manifestType,
              licenseUrl: resolvedStream.licenseUrl,
              licenseHeaders: resolvedStream.licenseHeaders,
              storyboardVttUrl: resolvedStream.storyboardVttUrl,
              chapters: resolvedStream.chapters,
              subtitles: resolvedStream.subtitles,
            })
        } catch {
          if (resolutionId === activeTrailerResolutionId) {
            resolvedTrailerMediaSource.value = null
          }
        }
      },
      { immediate: true },
    )
  }

  return { trailerUrl, trailerMediaSource, hasTrailer, isTrailerBlocked, resetForEntryLoad, observe }
}
