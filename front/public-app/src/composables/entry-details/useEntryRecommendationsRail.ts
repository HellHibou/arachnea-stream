import { onMounted, onBeforeUnmount, watch, type Ref } from 'vue'
import { useRouter } from 'vue-router'
import type { EntryDetails } from '@/types/entry'
import type { MediaItem } from '@/types/media'
import { mediaRoute } from '@/services/mediaRouting'
import { entryRecommendations } from './entryRecommendations'

interface EntryRecommendationsRailOptions {
  source: Ref<string>
  details: Ref<EntryDetails | null>
  rail: Readonly<Ref<{ $el: HTMLElement } | null>>
}

/**
 * Owns deferred recommendation observation and card navigation for one entry.
 * @param options Entry data and the component's recommendations-rail template ref.
 * @returns Recommendation state, pagination and selection actions.
 */
export function useEntryRecommendationsRail(options: EntryRecommendationsRailOptions) {
  const { source, details, rail: recommendationsRailRef } = options
  const router = useRouter()
  const recommendations = entryRecommendations({ source, details })
  const { loadRecommendationsOnVisible, showRecommendations } = recommendations
  /** Intersection observer triggering the deferred recommendations load. */
  let recommendationsObserver: IntersectionObserver | null = null

  onMounted(() => {
    recommendationsObserver = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          void loadRecommendationsOnVisible()
        }
      },
      { rootMargin: '320px 0px' },
    )
    const element = recommendationsRailRef.value?.$el
    if (showRecommendations.value && element) {
      recommendationsObserver.observe(element)
    }
  })

  onBeforeUnmount(() => {
    recommendationsObserver?.disconnect()
    recommendationsObserver = null
  })

  watch(
    [recommendationsRailRef, showRecommendations],
    ([rail, visible]) => {
      recommendationsObserver?.disconnect()
      const element = (rail as unknown as { $el?: HTMLElement } | null)?.$el ?? null
      if (!recommendationsObserver || !visible || !element) {
        return
      }
      recommendationsObserver.observe(element)
    },
    { flush: 'post' },
  )

  /**
   * Opens the entry details route for a selected recommendation card.
   *
   * @param item Recommendation card selected from the rail.
   */
  function handleRecommendationSelect(item: MediaItem) {
    const destination = mediaRoute(item)
    if (destination) void router.push(destination)
  }

  return { ...recommendations, handleRecommendationSelect }
}
