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
  const displayTitle = computed(() => options.details.value?.title?.trim() || t('entry.unavailableTitle'))

  const alternativeTitleLabel = computed(() => {
    const selectedEpisodeAlt = options.selectedEpisode.value?.alternativeTitleLabel?.trim()
    if (selectedEpisodeAlt) {
      return selectedEpisodeAlt
    }

    return options.details.value?.alternativeTitleLabel?.trim() || null
  })

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
        details?.heroImageUrl ??
        null,
      posterFrameUsesContain: Boolean(details?.logoUrl),
      heroBackgroundUrl: details?.imagePortraitUrl ?? details?.imagePosterUrl ?? details?.heroImageUrl ?? null,
      heroBackgroundPortraitUrl: details?.imagePortraitUrl ?? details?.imagePosterUrl ?? null,
      heroBackgroundLandscapeUrl: details?.imageLandscapeUrl ?? null,
      releaseDateLabel: selectedEpisode?.releaseDateLabel ?? details?.releaseDateLabel ?? null,
      expireLabel: selectedEpisode?.expireLabel ?? details?.expireLabel ?? null,
      durationLabel: selectedEpisode?.durationLabel ?? details?.durationLabel ?? null,
    }
  })

  return {
    displayTitle,
    alternativeTitleLabel,
    displayDescription: computed(() => activePageContext.value.description),
    posterFrameImageUrl: computed(() => activePageContext.value.posterFrameImageUrl),
    posterFrameUsesContain: computed(() => activePageContext.value.posterFrameUsesContain),
    heroBackgroundUrl: computed(() => activePageContext.value.heroBackgroundUrl),
    heroBackgroundPortraitUrl: computed(() => activePageContext.value.heroBackgroundPortraitUrl),
    heroBackgroundLandscapeUrl: computed(() => activePageContext.value.heroBackgroundLandscapeUrl),
    displayReleaseDateLabel: computed(() => activePageContext.value.releaseDateLabel),
    displayExpireLabel: computed(() => activePageContext.value.expireLabel),
    displayDurationLabel: computed(() => activePageContext.value.durationLabel),
  }
}
