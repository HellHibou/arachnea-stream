import { shallowRef, watch, type Ref } from 'vue'

import { searchMediaItemsPage } from '@/services/rustify'
import { t } from '@/i18n'
import type { MediaItem } from '@/types/media'

/**
 * Options accepted by the media search results controller.
 */
interface UseMediaSearchResultsOptions {
  /**
   * Reactive reference to the search query submitted by the parent controller.
   */
  submittedQuery: Ref<string>
  /**
   * Reactive reference to the monotonic key used to trigger a new search even when the submitted query did not change.
   */
  searchRequestId: Ref<number>
  /**
   * Reactive reference to the selected backend media types.
   */
  selectedMediaTypes: Ref<string[]>
  /**
   * Reactive reference to the selected backend themes.
   */
  selectedThemes: Ref<string[]>
}

/**
 * Owns backend search execution and the resulting UI state for the media search screen.
 *
 * @param options Reactive sources used to trigger searches from the parent controller.
 * @returns Search results, loading state, and error state derived from parent-triggered searches.
 */
export function mediaSearchResults(options: UseMediaSearchResultsOptions) {
  /** Reactive array of search result media items. */
  const mediaItems = shallowRef<MediaItem[]>([])
  /** Whether a search has been executed at least once. */
  const hasSearched = shallowRef(false)
  /** Whether a search is currently in progress. */
  const isSearching = shallowRef(false)
  /** Whether additional results are currently being loaded. */
  const isLoadingMore = shallowRef(false)
  /** The current page number of search results. */
  const currentPage = shallowRef(1)
  /** Whether there are more search results available. */
  const haveMore = shallowRef(false)
  /** Error message from the main search, or null if successful. */
  const errorMessage = shallowRef<string | null>(null)
  /** Error message from loading more results, or null if successful. */
  const loadMoreErrorMessage = shallowRef<string | null>(null)
  /** Source parameters from the last search, used for pagination. */
  const sourceParams = shallowRef<Record<string, string>[]>([])
  /** The last executed search query. */
  const latestQuery = shallowRef('')
  /** Request identifier counter for ignoring stale search responses. */
  const latestSearchRequestId = shallowRef(0)

  /**
   * Runs the backend search requested by the parent toolbar and updates the collection state.
   *
   * @param rawQuery - Raw search query provided by the parent component.
   */
  async function runSearch(rawQuery: string): Promise<void> {
    const requestId = latestSearchRequestId.value + 1
    latestSearchRequestId.value = requestId

    const query = rawQuery.trim()

    errorMessage.value = null

    if (!query) {
      hasSearched.value = false
      mediaItems.value = []
      isSearching.value = false
      isLoadingMore.value = false
      currentPage.value = 1
      haveMore.value = false
      sourceParams.value = []
      return
    }

    isSearching.value = true
    latestQuery.value = query
    loadMoreErrorMessage.value = null

    try {
      const nextPage = await searchMediaItemsPage(query, {
        mediaTypes: options.selectedMediaTypes.value,
        themes: options.selectedThemes.value,
      })

      if (requestId !== latestSearchRequestId.value) {
        return
      }

      mediaItems.value = nextPage.items
      currentPage.value = nextPage.currentPage
      haveMore.value = nextPage.haveMore
      sourceParams.value = nextPage.sourceParams
      hasSearched.value = true
    } catch (error) {
      if (requestId !== latestSearchRequestId.value) {
        return
      }

      mediaItems.value = []
      hasSearched.value = true
      haveMore.value = false
      sourceParams.value = []
      errorMessage.value =
        error instanceof Error ? error.message : t('errors.search')
    } finally {
      if (requestId === latestSearchRequestId.value) {
        isSearching.value = false
      }
    }
  }

  /**
   * Loads the next search page and appends new unique items to the current results.
   */
  async function loadMore(): Promise<void> {
    if (!latestQuery.value || !haveMore.value || isSearching.value || isLoadingMore.value) {
      return
    }

    const requestId = latestSearchRequestId.value
    isLoadingMore.value = true
    loadMoreErrorMessage.value = null

    try {
      const nextPage = await searchMediaItemsPage(
        latestQuery.value,
        {
          mediaTypes: options.selectedMediaTypes.value,
          themes: options.selectedThemes.value,
        },
        currentPage.value + 1,
        sourceParams.value,
      )

      if (requestId !== latestSearchRequestId.value) {
        return
      }

      mediaItems.value = mergeUniqueMediaItems(mediaItems.value, nextPage.items)
      currentPage.value = nextPage.currentPage
      haveMore.value = nextPage.haveMore
      sourceParams.value = nextPage.sourceParams
    } catch (error) {
      if (requestId !== latestSearchRequestId.value) {
        return
      }

      loadMoreErrorMessage.value =
        error instanceof Error ? error.message : t('errors.search')
    } finally {
      if (requestId === latestSearchRequestId.value) {
        isLoadingMore.value = false
      }
    }
  }

  /**
   * Appends only result cards that are not already visible.
   *
   * @param currentItems - Items already rendered by the search screen.
   * @param nextItems - Items returned by the next backend page.
   * @returns Combined array of unique media items.
   */
  function mergeUniqueMediaItems(currentItems: MediaItem[], nextItems: MediaItem[]): MediaItem[] {
    const seen = new Set(currentItems.map((item) => item.id))
    const mergedItems = [...currentItems]

    nextItems.forEach((item) => {
      if (seen.has(item.id)) {
        return
      }

      seen.add(item.id)
      mergedItems.push(item)
    })

    return mergedItems
  }

  watch(
    options.searchRequestId,
    () => {
      void runSearch(options.submittedQuery.value)
    },
    { immediate: true },
  )

  return {
    mediaItems,
    hasSearched,
    isSearching,
    isLoadingMore,
    haveMore,
    errorMessage,
    loadMoreErrorMessage,
    loadMore,
  }
}
