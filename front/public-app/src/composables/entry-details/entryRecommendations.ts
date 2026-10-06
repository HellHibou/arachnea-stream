import { computed, ref, shallowRef, watch, type Ref } from 'vue'

import { getRecommendations } from '@/services/rustify'
import type { EntryDetails } from '@/types/entry'
import type { MediaItem } from '@/types/media'

/**
 * Options accepted by the entry recommendations controller.
 */
interface UseEntryRecommendationsOptions {
  /**
   * Backend source used to resolve the current entry.
   */
  source: Ref<string>
  /**
   * Loaded entry details carrying the optional recommendations section.
   */
  details: Ref<EntryDetails | null>
}

/**
 * Owns deferred recommendations loading for one entry details view.
 *
 * Immediate cards from `get_entry` are kept as-is. When only a
 * `recommendations.link` is present, `get_recommendations` is called once the
 * rail becomes visible, then page by page while `haveMore` allows it. Stale
 * responses are ignored when the entry changes.
 *
 * @param options Reactive source and details used to keep recommendations in sync with the current entry.
 * @returns Recommendations state and actions for the rail.
 */
export function entryRecommendations(options: UseEntryRecommendationsOptions) {
  /** Recommendation cards accumulated for the current entry. */
  const items = ref<MediaItem[]>([])
  /** Whether the initial deferred load is in progress. */
  const isLoading = shallowRef(false)
  /** Whether an additional page is being loaded. */
  const isLoadingMore = shallowRef(false)
  /** Error message from recommendations loading, or null if successful. */
  const errorMessage = shallowRef<string | null>(null)
  /** Current page number for deferred pagination. */
  const currentPage = shallowRef(1)
  /** Whether a next page is available through `get_recommendations`. */
  const haveMore = shallowRef(false)
  /** Whether the deferred link has been resolved for the current entry. */
  const hasLoadedLink = shallowRef(false)

  /** Request identifier counter to ignore stale responses. */
  let latestRequestId = 0

  /** Deferred recommendations link for the current entry, if any. */
  const recommendationsLink = computed(() => options.details.value?.recommendations?.link ?? null)

  /** Whether the rail should be displayed for the current entry. */
  const showRecommendations = computed(
    () => items.value.length > 0 || isLoading.value || errorMessage.value !== null,
  )

  /**
   * Resets recommendations state whenever the featured entry changes and seeds immediate cards.
   */
  function resetForEntry(): void {
    latestRequestId += 1
    const section = options.details.value?.recommendations
    items.value = [...(section?.items ?? [])]
    currentPage.value = section?.currentPage ?? 1
    haveMore.value = section?.haveMore ?? false
    errorMessage.value = null
    isLoading.value = false
    isLoadingMore.value = false
    hasLoadedLink.value = false
  }

  /**
   * Loads one deferred recommendations page and appends new cards without duplicates.
   *
   * @param page 1-based page number requested from the backend.
   * @param append Whether cards are appended (`true`) or replace the deferred list (`false`).
   */
  async function loadRecommendationsPage(page: number, append: boolean): Promise<void> {
    const link = recommendationsLink.value
    if (!link) {
      return
    }

    const requestId = ++latestRequestId
    if (append) {
      isLoadingMore.value = true
    } else {
      isLoading.value = true
      errorMessage.value = null
    }

    try {
      const result = await getRecommendations(options.source.value, link, page)
      if (requestId !== latestRequestId) {
        return
      }

      const knownIds = new Set(items.value.map((item) => item.id))
      const freshItems = result.items.filter((item) => !knownIds.has(item.id))
      items.value = append ? [...items.value, ...freshItems] : [...items.value, ...freshItems]
      currentPage.value = result.currentPage
      haveMore.value = result.haveMore
      hasLoadedLink.value = true
    } catch (error) {
      if (requestId !== latestRequestId) {
        return
      }
      errorMessage.value = error instanceof Error ? error.message : String(error)
      hasLoadedLink.value = true
    } finally {
      if (requestId === latestRequestId) {
        isLoading.value = false
        isLoadingMore.value = false
      }
    }
  }

  /**
   * Triggers the deferred load once the rail becomes visible, at most once per entry.
   * Items already received in `get_entry` are preserved; the deferred call only
   * resolves a `link` when no immediate cards were provided.
   */
  async function loadRecommendationsOnVisible(): Promise<void> {
    if (hasLoadedLink.value || isLoading.value || !recommendationsLink.value) {
      return
    }
    if (items.value.length > 0) {
      hasLoadedLink.value = true
      return
    }
    await loadRecommendationsPage(Math.max(1, currentPage.value), true)
  }

  /**
   * Loads the next deferred page when available.
   */
  async function handleLoadMoreRecommendations(): Promise<void> {
    if (!haveMore.value || isLoadingMore.value || isLoading.value || !recommendationsLink.value) {
      return
    }
    await loadRecommendationsPage(currentPage.value + 1, true)
  }

  /**
   * Retries the deferred load after a failure.
   */
  async function handleRetryRecommendations(): Promise<void> {
    hasLoadedLink.value = false
    errorMessage.value = null
    await loadRecommendationsOnVisible()
  }

  watch(
    () => options.details.value,
    () => {
      resetForEntry()
    },
    { immediate: true },
  )

  return {
    recommendationItems: items,
    isRecommendationsLoading: isLoading,
    isRecommendationsLoadingMore: isLoadingMore,
    recommendationsErrorMessage: errorMessage,
    hasMoreRecommendations: haveMore,
    showRecommendations,
    loadRecommendationsOnVisible,
    handleLoadMoreRecommendations,
    handleRetryRecommendations,
  }
}
