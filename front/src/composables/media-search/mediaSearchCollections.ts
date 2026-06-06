import { computed, type Ref } from 'vue'

import type { MediaItem } from '@/types/media'

/**
 * Collection group rendered by the media search view.
 */
export interface MediaSearchCollectionGroup {
  key: string
  label: string | undefined
  items: MediaItem[]
}

/**
 * Options accepted by the media search collection mapper.
 */
interface UseMediaSearchCollectionsOptions {
  /**
   * Search results currently exposed by the backend search controller.
   */
  mediaItems: Ref<MediaItem[]>
  /**
   * Optional label displayed above the media card collection.
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
