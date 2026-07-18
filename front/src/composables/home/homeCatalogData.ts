import { computed, shallowRef, toRef, watch, type MaybeRefOrGetter } from 'vue'

import {
  getCategoryCatalog,
  loadHomeCatalog,
  loadInitialHomeSectionPages,
} from '@/services/rustify'
import { loadDeferredBannerPages } from '@/composables/home/deferredBannerLoader'
import { t } from '@/i18n'
import type { HomeCatalogData, HomeCategory } from '@/types/home'

/**
 * Options accepted by the home catalog loader.
 */
interface UseHomeCatalogDataOptions {
  /**
   * Active catalog mode.
   */
  mode: MaybeRefOrGetter<'home' | 'category'>
  /**
   * Selected aggregated category when the catalog runs in category mode.
   */
  category: MaybeRefOrGetter<HomeCategory | null>
}

/**
 * Loads and caches the backend payload rendered by the home and category screens.
 *
 * @param options Reactive mode and category sources that drive backend requests.
 * @returns Catalog payload, loading state, and error state for the current screen.
 */
export function homeCatalogData(options: UseHomeCatalogDataOptions) {
  /** Reactive reference to the catalog mode. */
  const mode = toRef(options.mode)
  /** Reactive reference to the selected category. */
  const category = toRef(options.category)
  /** Reactive reference to the loaded catalog data. */
  const catalog = shallowRef<HomeCatalogData | null>(null)
  /** Whether the catalog is currently being loaded. */
  const isLoading = shallowRef(false)
  /** Whether the catalog has been loaded at least once. */
  const hasLoaded = shallowRef(false)
  /** Error message from catalog loading, or null if successful. */
  const errorMessage = shallowRef<string | null>(null)
  /** Request identifier counter for ignoring stale responses. */
  const latestRequestId = shallowRef(0)

  /**
   * Cache key derived from the current mode and category.
   * Used to detect when a new request is needed.
   *
   * @returns A string key identifying the current catalog request.
   */
  const requestKey = computed(() => {
    if (mode.value !== 'category') {
      return 'home'
    }

    return JSON.stringify({
      id: category.value?.id ?? null,
      mergeKey: category.value?.mergeKey ?? null,
      sources: (category.value?.sources ?? []).map((source) =>
        Object.entries(source).sort(([left], [right]) => left.localeCompare(right)),
      ),
    })
  })

  /**
   * Fetches the payload matching the current mode and ignores stale responses.
   * Loads either the home catalog or a category-specific catalog based on the mode.
   */
  async function loadCatalog(): Promise<void> {
    const requestId = latestRequestId.value + 1
    latestRequestId.value = requestId
    errorMessage.value = null
    catalog.value = null
    isLoading.value = true

    try {
      const loadedCatalog =
        mode.value === 'category' && category.value
          ? await getCategoryCatalog(category.value)
          : await loadHomeCatalog()
      const catalogWithInitialSections = await loadInitialHomeSectionPages(loadedCatalog)
      const nextCatalog = await loadDeferredBannerPages(catalogWithInitialSections)

      if (requestId !== latestRequestId.value) {
        return
      }

      catalog.value = nextCatalog
      hasLoaded.value = true
    } catch (error) {
      if (requestId !== latestRequestId.value) {
        return
      }

      catalog.value = null
      hasLoaded.value = true
      errorMessage.value =
        error instanceof Error ? error.message : t('errors.homeCatalog')
    } finally {
      if (requestId === latestRequestId.value) {
        isLoading.value = false
      }
    }
  }

  watch(requestKey, () => {
    void loadCatalog()
  }, { immediate: true })

  return {
    catalog,
    isLoading,
    hasLoaded,
    errorMessage,
  }
}
