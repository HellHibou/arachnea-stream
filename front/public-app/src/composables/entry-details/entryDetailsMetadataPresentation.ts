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
  /** Badge metadata labels such as season count. */
  const metadataBadges = computed(() =>
    [options.details.value?.seasonCountLabel].filter((value): value is string => Boolean(value)),
  )

  /**
   * Genre labels from the entry, falling back to theme labels if genres are not available.
   *
   * @returns Array of genre labels.
   */
  const genreLabels = computed(() => {
    const details = options.details.value

    return details?.genreLabels.length ? details.genreLabels : (details?.themeLabels ?? [])
  })

  /** Comma-separated string of up to 4 genre labels, or null if empty. */
  const genreText = computed(() => genreLabels.value.slice(0, 4).join(', ') || null)

  /**
   * Topic chips derived from theme labels, excluding those already shown as genres.
   *
   * @returns Array of unique topic labels not already in genre labels.
   */
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

  /** The entry score or rating, or null if not available. */
  const score = computed(() => options.details.value?.score ?? null)

  return {
    metadataBadges,
    genreText,
    topicChips,
    /** Comma-separated string of up to 6 casting labels, or null if empty. */
    castingText: computed(() =>
      options.details.value?.castingLabels.slice(0, 6).join(', ') || null,
    ),
    /** Comma-separated string of up to 6 director labels, or null if empty. */
    directorText: computed(() =>
      options.details.value?.directorLabels.slice(0, 6).join(', ') || null,
    ),
    score,
  }
}
