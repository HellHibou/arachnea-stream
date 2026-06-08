import { shallowRef, watch, type Ref } from 'vue'

import { searchMediaItemsPage } from '@/services/rustify'
import { t } from '@/i18n'
import type { MediaItem } from '@/types/media'

/**
 * Options accepted by the media search results controller.
 */
interface UseMediaSearchResultsOptions {
  /**
   * Search query submitted by the parent controller.
   */
  submittedQuery: Ref<string>
  /**
   * Monotonic key used to trigger a new search even when the submitted query did not change.
   */
  searchRequestId: Ref<number>
  /**
   * Selected backend media types.
   */
  selectedMediaTypes: Ref<string[]>
  /**
   * Selected backend themes.
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
  const mediaItems = shallowRef<MediaItem[]>([])
  const hasSearched = shallowRef(false)
  const isSearching = shallowRef(false)
  const isLoadingMore = shallowRef(false)
  const currentPage = shallowRef(1)
  const haveMore = shallowRef(false)
  const errorMessage = shallowRef<string | null>(null)
  const loadMoreErrorMessage = shallowRef<string | null>(null)
  const sourceParams = shallowRef<Record<string, string>[]>([])
  const latestQuery = shallowRef('')
  const latestSearchRequestId = shallowRef(0)

  /**
   * Runs the backend search requested by the parent toolbar and updates the collection state.
   *
   * @param rawQuery Raw search query provided by the parent component.
   */
  async function runSearch(rawQuery: string) {
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
  async function loadMore() {
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
   * @param currentItems Items already rendered by the search screen.
   * @param nextItems Items returned by the next backend page.
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
