import { computed, type Ref } from 'vue'

import type { EntryDetails } from '@/types/entry'

/**
 * Options accepted by the entry details metadata presentation mapper.
 */
interface UseEntryDetailsMetadataPresentationOptions {
  /**
   * Loaded detailed entry rendered by the view.
   */
  details: Ref<EntryDetails | null>
}

/**
 * Provides display-ready metadata groups used by the facts panel.
 *
 * @param options Reactive data sources already normalized by the feature composables.
 * @returns Pure computed values consumed by the metadata subcomponent.
 */
export function entryDetailsMetadataPresentation(
  options: UseEntryDetailsMetadataPresentationOptions,
) {
  const metadataBadges = computed(() =>
    [options.details.value?.seasonCountLabel].filter((value): value is string => Boolean(value)),
  )

  const genreLabels = computed(() => {
    const details = options.details.value

    return details?.genreLabels.length ? details.genreLabels : (details?.themeLabels ?? [])
  })

  const genreText = computed(() => genreLabels.value.slice(0, 4).join(', ') || null)

  const topicChips = computed(() => {
    const seen = new Set<string>()
    const excluded = new Set(genreLabels.value.map((value) => value.toLocaleLowerCase()))
    const values = options.details.value?.themeLabels ?? []

    return values.filter((value) => {
      const key = value.toLocaleLowerCase()

      if (excluded.has(key) || seen.has(key)) {
        return false
      }

      seen.add(key)
      return true
    })
  })

  const score = computed(() => options.details.value?.score ?? null)

  return {
    metadataBadges,
    genreText,
    topicChips,
    castingText: computed(() =>
      options.details.value?.castingLabels.slice(0, 6).join(', ') || null,
    ),
    directorText: computed(() =>
      options.details.value?.directorLabels.slice(0, 6).join(', ') || null,
    ),
    score,
  }
}
