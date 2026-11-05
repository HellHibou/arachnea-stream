import { computed, type Ref } from 'vue'
import type { ResolvedPlayerMediaSource } from '@/services/players'
import type { MediaItem } from '@/types/media'
import type { EntryBookmark } from '@/services/storage'
import { resolveImageUrl } from '@/composables/media/useFailedImageUrls'
import { toEntryEpisodeMediaItem } from './entryDetailsMediaItems'
import type { entryDetailsData } from './entryDetailsData'
import type { entryEpisodeSelection } from './entryEpisodeSelection'
import type { entryVideoPlayer } from './entryVideoPlayer'

interface EntryMediaSurfaceOptions {
  source: Ref<string>
  useTrailerAsBackground: Readonly<Ref<boolean>>
  data: Pick<ReturnType<typeof entryDetailsData>, 'details'>
  selection: Pick<ReturnType<typeof entryEpisodeSelection>, 'selectedEpisode' | 'previousNavigableEpisode' | 'nextNavigableEpisode'>
  player: Pick<ReturnType<typeof entryVideoPlayer>, 'trailerMediaSource' | 'resolvedPlayerPosterUrl'>
  currentEntryBookmark: Ref<EntryBookmark | null>
}

/**
 * Derives background/trailer media, artwork, adjacent previews and restored playback time.
 * @param options Reactive feature outputs and current background/bookmark preferences.
 * @returns Presentation refs consumed by the entry details surface.
 */
export function useEntryMediaSurface(options: EntryMediaSurfaceOptions) {
  const { source, currentEntryBookmark } = options
  const { details } = options.data
  const { selectedEpisode, previousNavigableEpisode, nextNavigableEpisode } = options.selection
  const { trailerMediaSource, resolvedPlayerPosterUrl } = options.player
  /** Media item previewed on the player's previous episode control. */
  const previousPlayableItem = computed<MediaItem | null>(() =>
    previousNavigableEpisode.value
      ? toEntryEpisodeMediaItem(previousNavigableEpisode.value, source.value)
      : null,
  )

  /** Media item previewed on the player's next episode control. */
  const nextPlayableItem = computed<MediaItem | null>(() =>
    nextNavigableEpisode.value
      ? toEntryEpisodeMediaItem(nextNavigableEpisode.value, source.value)
      : null,
  )

  /**
   * Preserves the resolved trailer manifest and DRM data for the full-page background.
   */
  const backgroundVideoSource = computed<ResolvedPlayerMediaSource | null>(() =>
    options.useTrailerAsBackground.value
      ? trailerMediaSource.value
      : null,
  )

  /**
   * Exposes the playback position that should be restored for the active episode video.
   */
  const initialPlaybackTime = computed(() => {
    const bookmark = currentEntryBookmark.value
    const currentEpisodeId = selectedEpisode.value?.id ?? null

    if (!bookmark || bookmark.selectedItemId !== currentEpisodeId) {
      return null
    }

    return bookmark.playbackTime
  })

  /**
   * Exposes the selected player title image, falling back to episode or entry imagery.
   *
   * Failed image URLs behave exactly like a `null` candidate: the fallback chain
   * advances to the next available image.
   */
  const mediaPosterUrl = computed(() =>
    resolveImageUrl([
      resolvedPlayerPosterUrl.value,
      selectedEpisode.value?.previewUrl,
      details.value?.imageLandscapeUrl,
      details.value?.imagePosterUrl,
    ]),
  )

  /**
   * Exposes the entry poster used as the initial poster for trailers in the integrated Video.js player.
   */
  const trailerPosterUrl = computed(() => resolveImageUrl([details.value?.imagePosterUrl]))

  /**
   * Exposes the logo overlaid on top of the media poster in the integrated Video.js player.
   */
  const mediaOverlayLogoUrl = computed(() => resolveImageUrl([details.value?.logoUrl]))

  return { previousPlayableItem, nextPlayableItem, backgroundVideoSource, initialPlaybackTime, mediaPosterUrl, trailerPosterUrl, mediaOverlayLogoUrl }
}
