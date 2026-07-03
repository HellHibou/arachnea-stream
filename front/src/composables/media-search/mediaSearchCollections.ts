import { computed, type Ref } from 'vue'

import type { MediaItem } from '@/types/media'

/**
 * Collection group rendered by the media search view.
 */
export interface MediaSearchCollectionGroup {
  /** Unique key identifying the collection group. */
  key: string
  /** Display label shown above the collection, or undefined for no label. */
  label: string | undefined
  /** Media items belonging to this collection group. */
  items: MediaItem[]
}

/**
 * Options accepted by the media search collection mapper.
 */
interface UseMediaSearchCollectionsOptions {
  /**
   * Reactive reference to the search results currently exposed by the backend search controller.
   */
  mediaItems: Ref<MediaItem[]>
  /**
   * Reactive reference to the optional label displayed above the media card collection.
   */
  collectionLabel: Ref<string | undefined>
}

/**
 * Builds the merged result collection rendered by the search screen.
 *
 * @param options Reactive sources used to organize visible search results.
 * @returns Merged collection ready to render.
 */
export function mediaSearchCollections(options: UseMediaSearchCollectionsOptions) {
  /**
   * All visible collection groups for the search results.
   * Currently always returns a single group with all results.
   *
   * @returns Array containing one collection group with all search results.
   */
  const visibleCollections = computed<MediaSearchCollectionGroup[]>(() => [
    {
      key: 'all-results',
      label: options.collectionLabel.value,
      items: options.mediaItems.value,
    },
  ])

  return {
    visibleCollections,
  }
}
