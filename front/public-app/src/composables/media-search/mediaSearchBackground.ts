import { computed, type Ref } from 'vue'

import { useStorage } from '@/services/storage'
import type { BackgroundMediaCandidate, MediaItem } from '@/types/media'

/** Maximum number of background media candidates exposed to the background layer. */
const BACKGROUND_MEDIA_ITEMS_LIMIT = 10

/**
 * Options accepted by the media search background controller.
 */
interface UseMediaSearchBackgroundOptions {
  /**
   * Reactive reference to the search results currently exposed by the backend search controller.
   */
  mediaItems: Ref<MediaItem[]>
}

/**
 * Returns the first available thumbnail image URL for one media item.
 * Prefers landscape images for the full-page background, then falls back to the generic and poster URLs.
 *
 * @param item Media item rendered in the search results collection.
 * @returns Trimmed image URL, or null when no image is available.
 */
function toBackgroundImageUrl(item: MediaItem): string | null {
  return item.imageLandscapeUrl?.trim() || item.imageUrl?.trim() || item.imagePosterUrl?.trim() || null
}

/**
 * Resolves the background media candidates exposed by the search screen.
 *
 * @param options Reactive sources describing the current search results.
 * @returns Background media candidates derived from search result thumbnails.
 */
export function mediaSearchBackground(options: UseMediaSearchBackgroundOptions) {
  /** Application parameters from storage. */
  const parameters = useStorage().getParameters()

  /**
   * Background media candidates derived from the first search result thumbnails.
   * Deduplicates image URLs and stops at {@link BACKGROUND_MEDIA_ITEMS_LIMIT} entries.
   *
   * @returns Array of background media candidates from search result thumbnails.
   */
  const backgroundMediaItems = computed<BackgroundMediaCandidate[]>(() => {
    if (!parameters.useCatalogBannersAsBackground.value) {
      return []
    }

    const items: BackgroundMediaCandidate[] = []
    const uniqueImageUrls = new Set<string>()

    for (const mediaItem of options.mediaItems.value) {
      if (items.length >= BACKGROUND_MEDIA_ITEMS_LIMIT) {
        break
      }

      const imageUrl = toBackgroundImageUrl(mediaItem)

      if (!imageUrl || uniqueImageUrls.has(imageUrl)) {
        continue
      }

      uniqueImageUrls.add(imageUrl)
      items.push({ imageUrl, videoUrl: null })
    }

    return items
  })

  return {
    backgroundMediaItems,
  }
}
