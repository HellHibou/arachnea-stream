import { computed, type Ref } from 'vue'

import {
  resolveBackgroundMediaSource,
  type ResolvedPlayerMediaSource,
} from '@/services/players'
import type { BackgroundMediaCandidate, ThumbnailImageFit } from '@/types/media'

/** Union type representing a resolved background media item, either an image with a source URL or a video with a resolved media source. */
export type ResolvedBackgroundMediaItem =
  | { type: 'image'; src: string }
  | { type: 'video'; source: ResolvedPlayerMediaSource }

/**
 * Options accepted by the background media controller.
 */
interface UseBackgroundMediaOptions {
  /**
   * Optional decorative background video rendered behind the whole page.
   */
  videoUrl: Ref<string | null>
  /**
   * Optional image rendered behind the whole page.
   */
  imageUrl: Ref<string | null>
  /**
   * Optional portrait-oriented image rendered behind the whole page.
   */
  imagePortraitUrl: Ref<string | null>
  /**
   * Optional landscape-oriented image rendered behind the whole page.
   */
  imageLandscapeUrl: Ref<string | null>
  /**
   * Optional images rendered behind the whole page.
   */
  imageUrls: Ref<string[]>
  /**
   * Optional ordered media candidates rendered behind the whole page.
   */
  mediaItems: Ref<BackgroundMediaCandidate[]>
  /**
   * Enables a decorative motion effect on the background media.
   */
  isAnimated: Ref<boolean>
  /**
   * Controls whether the background image should be fully visible or cropped.
   */
  imageFit: Ref<ThumbnailImageFit>
}

/**
 * Normalizes a background URL string by trimming whitespace.
 *
 * @param value - The URL string to normalize.
 * @returns The trimmed URL string, or null if the value is empty or not a string.
 */
function normalizeBackgroundUrl(value: string | null | undefined): string | null {
  return typeof value === 'string' && value.trim().length > 0
    ? value.trim()
    : null
}

/**
 * Creates a unique key for a resolved background media item for deduplication.
 *
 * @param item - The resolved background media item.
 * @returns A string key identifying the media item type and source.
 */
function createBackgroundMediaKey(item: ResolvedBackgroundMediaItem): string {
  if (item.type === 'video') {
    return `video:${item.source.renderer}:${item.source.src}`
  }

  return `image:${item.src}`
}

/**
 * Resolves a background media candidate into a resolved media item.
 * Prioritizes video sources over images.
 *
 * @param candidate - The background media candidate to resolve.
 * @returns The resolved background media item, or null if no valid media is found.
 */
function resolveBackgroundCandidate(
  candidate: BackgroundMediaCandidate,
): ResolvedBackgroundMediaItem | null {
  const videoUrl = normalizeBackgroundUrl(candidate.videoUrl)

  if (videoUrl) {
    const source = resolveBackgroundMediaSource(videoUrl)

    if (source) {
      return { type: 'video', source }
    }
  }

  const imageUrl = normalizeBackgroundUrl(candidate.imageUrl)

  if (imageUrl) {
    return { type: 'image', src: imageUrl }
  }

  return null
}

/**
 * Resolves the active background media and the derived rendering state.
 *
 * @param options Reactive sources describing the requested background media.
 * @returns Computed values used by the background view layer.
 */
export function backgroundMedia(options: UseBackgroundMediaOptions) {
  /**
   * Returns the most appropriate image URL based on viewport orientation.
   * - If width > height: landscape first, then portrait
   * - If height > width: portrait first, then landscape
   *
   * @returns The selected image URL based on viewport orientation.
   */
  const selectedOrientationImageUrl = computed(() => {
    const portraitUrl = options.imagePortraitUrl.value
    const landscapeUrl = options.imageLandscapeUrl.value

    if (typeof window === 'undefined') {
      return portraitUrl ?? landscapeUrl
    }

    const isLandscapeViewport = window.innerWidth > window.innerHeight

    if (isLandscapeViewport) {
      return landscapeUrl ?? portraitUrl
    }

    return portraitUrl ?? landscapeUrl
  })

  /**
   * Resolves every background media candidate in display order.
   *
   * @returns Array of resolved background media items with duplicates removed.
   */
  const backgroundMediaItems = computed<ResolvedBackgroundMediaItem[]>(() => {
    const primaryImageUrl =
      normalizeBackgroundUrl(options.imageUrl.value) ??
      normalizeBackgroundUrl(selectedOrientationImageUrl.value)
    const candidates: BackgroundMediaCandidate[] = [
      {
        videoUrl: options.videoUrl.value,
        imageUrl: primaryImageUrl,
      },
      ...options.mediaItems.value,
      ...options.imageUrls.value.map((imageUrl) => ({
        imageUrl,
        videoUrl: null,
      })),
    ]
    const uniqueKeys = new Set<string>()
    const items: ResolvedBackgroundMediaItem[] = []

    candidates.forEach((candidate) => {
      const item = resolveBackgroundCandidate(candidate)

      if (!item) {
        return
      }

      const key = createBackgroundMediaKey(item)

      if (uniqueKeys.has(key)) {
        return
      }

      uniqueKeys.add(key)
      items.push(item)
    })

    return items
  })

  /**
   * Indicates whether the background is currently rendered without media.
   *
   * @returns True when there are no background media items to display.
   */
  const isFallbackBackground = computed(() =>
    backgroundMediaItems.value.length === 0,
  )

  /**
   * Exposes the CSS modifiers applied to the optional background image.
   * When animation is active, forces cover mode for proper Ken Burns effect.
   *
   * @returns Object containing CSS class modifiers for the background image.
   */
  const backgroundImageClasses = computed(() => {
    const isAnimated = options.isAnimated.value
    // When animated, force cover mode for proper cropping during Ken Burns animation
    const imageFit = isAnimated ? 'cover' : options.imageFit.value

    return {
      'background__image--animated': isAnimated,
      'background__image--static': !isAnimated,
      'background__image--contain': imageFit === 'contain',
      'background__image--cover': imageFit === 'cover',
    }
  })

  return {
    backgroundMediaItems,
    isFallbackBackground,
    backgroundImageClasses,
  }
}
