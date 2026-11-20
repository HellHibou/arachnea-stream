import { shallowRef, watch } from 'vue'
import { createCategoryCatalogResolver, withCategoryOrigins, type CategoryContext } from '@/services/customHomeCategoryResolver'
import { discoverCategoryReferences } from '@/services/customHomeQuickAdd'
import type { CategoryReference } from '@/types/customHome'
import type { HomeCategory } from '@/types/home'

/** @param selected Category route input. @returns Verified add targets and navigation paths for its children. */
export function useCategoryHomeAdd(selected: () => HomeCategory | null) {
  const references = shallowRef<CategoryReference[]>([])
  const contexts = shallowRef<CategoryContext[]>([])
  watch(selected, async (category, _previous, onCleanup) => {
    references.value = []
    contexts.value = []
    if (!category) return
    let active = true
    onCleanup(() => { active = false })
    try {
      const resolver = createCategoryCatalogResolver()
      const current = await discoverCategoryReferences([category], resolver)
      if (!active) return
      references.value = current
      const result = await Promise.allSettled(current.map(async (reference) => ({
        reference, catalog: (await resolver.load(reference)).catalog,
      })))
      if (active) contexts.value = result.flatMap((entry) => entry.status === 'fulfilled' ? [entry.value] : [])
    } catch {
      // Navigation also supports collection routes with no source-home origin.
    }
  }, { immediate: true })

  /** @param category Displayed child. @returns Navigation hints discovered for the current route. */
  function withOrigins(category: HomeCategory): HomeCategory {
    return withCategoryOrigins(category, contexts.value)
  }

  return { references, withOrigins }
}
