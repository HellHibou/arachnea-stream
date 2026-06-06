import { computed } from 'vue'

import { useI18n } from '@/i18n'
import { mediaTypeValues, type SearchFilterOption } from '@/services/rustify'
import { useServiceMetadata } from './useServiceMetadata'

/**
 * Builds localized search filter options from static media types and service metadata.
 */
export function useSearchFilterOptions() {
  const { services } = useServiceMetadata()
  const { resolvedLanguage, t, translateTheme } = useI18n()

  const allFilterOption = computed<SearchFilterOption>(() => ({
    value: '',
    label: t('filters.all'),
  }))

  const mediaTypeFilterOptions = computed<SearchFilterOption[]>(() => [
    allFilterOption.value,
    ...mediaTypeValues
      .map((value) => ({
        value,
        label: t(`mediaTypes.${value}`),
      }))
      .sort((left, right) => left.label.localeCompare(right.label, resolvedLanguage.value)),
  ])

  const themeFilterOptions = computed<SearchFilterOption[]>(() => {
    const themeValues = new Set<string>()

    services.value.forEach((service) => {
      service.themes.forEach((theme) => {
        if (theme.code) {
          themeValues.add(theme.code)
        }
      })
    })

    return [
      allFilterOption.value,
      ...Array.from(themeValues)
        .map((value) => ({
          value,
          label: translateTheme(value),
        }))
        .sort((left, right) => left.label.localeCompare(right.label, resolvedLanguage.value)),
    ]
  })

  return {
    allFilterOption,
    mediaTypeFilterOptions,
    themeFilterOptions,
  }
}
