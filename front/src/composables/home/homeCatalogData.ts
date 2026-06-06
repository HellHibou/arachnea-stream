import { computed, shallowRef, toRef, watch, type MaybeRefOrGetter } from 'vue'

import { getCategoryCatalog, loadHomeCatalog } from '@/services/rustify'
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
  const mode = toRef(options.mode)
  const category = toRef(options.category)
  const catalog = shallowRef<HomeCatalogData | null>(null)
  const isLoading = shallowRef(false)
  const hasLoaded = shallowRef(false)
  const errorMessage = shallowRef<string | null>(null)
  const latestRequestId = shallowRef(0)

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
   */
  async function loadCatalog() {
    const requestId = latestRequestId.value + 1
    latestRequestId.value = requestId
    errorMessage.value = null
    catalog.value = null
    isLoading.value = true

    try {
      const nextCatalog =
        mode.value === 'category' && category.value
          ? await getCategoryCatalog(category.value)
          : await loadHomeCatalog()

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
