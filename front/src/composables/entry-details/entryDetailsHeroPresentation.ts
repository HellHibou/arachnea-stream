import { computed, type Ref } from 'vue'

import { t } from '@/i18n'
import type { EntryDetails, EntryEpisode } from '@/types/entry'

/**
 * Options accepted by the entry details hero presentation mapper.
 */
interface UseEntryDetailsHeroPresentationOptions {
  /**
   * Loaded detailed entry rendered by the view.
   */
  details: Ref<EntryDetails | null>
  /**
   * Episode currently selected in the built-in player.
   */
  selectedEpisode: Ref<EntryEpisode | null>
}

/**
 * Provides the title, description, imagery, and fact labels used in the hero area.
 *
 * @param options Reactive data sources already normalized by the feature composables.
 * @returns Pure computed values consumed by the hero-related subcomponents.
 */
export function entryDetailsHeroPresentation(options: UseEntryDetailsHeroPresentationOptions) {
  /** The display title from entry details or a fallback translation. */
  const displayTitle = computed(() => options.details.value?.title?.trim() || t('entry.unavailableTitle'))

  /**
   * The alternative title label, preferring the selected episode's label if available.
   *
   * @returns The alternative title label or null if not available.
   */
  const alternativeTitleLabel = computed(() => {
    const selectedEpisodeAlt = options.selectedEpisode.value?.alternativeTitleLabel?.trim()
    if (selectedEpisodeAlt) {
      return selectedEpisodeAlt
    }

    return options.details.value?.alternativeTitleLabel?.trim() || null
  })

  /**
   * Context object containing all display data for the hero section.
   * Combines episode and entry details, preferring episode data when available.
   *
   * @returns Object with description, images, and metadata labels for the hero area.
   */
  const activePageContext = computed(() => {
    const details = options.details.value
    const selectedEpisode = options.selectedEpisode.value

    return {
      description:
        selectedEpisode?.description?.trim() ||
        details?.description?.trim() ||
        t('entry.noDescription'),
      posterFrameImageUrl:
        details?.logoUrl ??
        details?.imageLandscapeUrl ??
        details?.imagePosterUrl ??
        details?.imageUrl ??
        details?.heroImageUrl ??
        null,
      posterFrameUsesContain: Boolean(details?.logoUrl),
      heroBackgroundUrl: details?.imagePortraitUrl ?? details?.imagePosterUrl ?? details?.heroImageUrl ?? details?.imageUrl ?? null,
      heroBackgroundPortraitUrl: details?.imagePortraitUrl ?? details?.imagePosterUrl ?? details?.imageUrl ?? null,
      heroBackgroundLandscapeUrl: details?.imageLandscapeUrl ?? details?.imageUrl ?? null,
      releaseDateLabel: selectedEpisode?.releaseDateLabel ?? details?.releaseDateLabel ?? null,
      expireLabel: selectedEpisode?.expireLabel ?? details?.expireLabel ?? null,
      durationLabel: selectedEpisode?.durationLabel ?? details?.durationLabel ?? null,
    }
  })

  return {
    displayTitle,
    alternativeTitleLabel,
    /** The description to display, from episode or entry details. */
    displayDescription: computed(() => activePageContext.value.description),
    /** The poster frame image URL to display. */
    posterFrameImageUrl: computed(() => activePageContext.value.posterFrameImageUrl),
    /** Whether the poster frame should use CSS contain mode. */
    posterFrameUsesContain: computed(() => activePageContext.value.posterFrameUsesContain),
    /** The hero background image URL. */
    heroBackgroundUrl: computed(() => activePageContext.value.heroBackgroundUrl),
    /** The hero background portrait image URL. */
    heroBackgroundPortraitUrl: computed(() => activePageContext.value.heroBackgroundPortraitUrl),
    /** The hero background landscape image URL. */
    heroBackgroundLandscapeUrl: computed(() => activePageContext.value.heroBackgroundLandscapeUrl),
    /** The release date label to display. */
    displayReleaseDateLabel: computed(() => activePageContext.value.releaseDateLabel),
    /** The expiry date label to display. */
    displayExpireLabel: computed(() => activePageContext.value.expireLabel),
    /** The duration label to display. */
    displayDurationLabel: computed(() => activePageContext.value.durationLabel),
  }
}
