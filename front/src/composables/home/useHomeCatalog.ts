import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  shallowRef,
  toRef,
  watch,
  type MaybeRefOrGetter,
} from 'vue'

import { loadHomeSectionPage } from '@/services/rustify'
import { useStorage, type HomePreferences } from '@/services/storage'
import type { HomeCatalogData, HomeCategory, HomeSection } from '@/types/home'
import type {
  BackgroundMediaCandidate,
  MediaCardCollectionMode,
  ThumbnailImageFit,
  ThumbnailOrientation,
} from '@/types/media'
import { homeCatalogData } from '@/composables/home/homeCatalogData'
import { t } from '@/i18n'

interface UseHomeCatalogOptions {
  /**
   * Active catalog mode.
   */
  mode: MaybeRefOrGetter<'home' | 'category'>
  /**
   * Selected aggregated category when the catalog runs in category mode.
   */
  category: MaybeRefOrGetter<HomeCategory | null>
  /**
   * Receives the ordered banner background image candidates whenever the current catalog changes.
   */
  onBackgroundMediaItemsChange?: (mediaItems: BackgroundMediaCandidate[]) => void
}

/**
 * Coordinates home catalog data, deferred section loading, and per-section preferences.
 *
 * @param options Reactive catalog mode, category, and optional background URL callback.
 * @returns State and actions consumed by the home catalog view.
 */
export function useHomeCatalog(options: UseHomeCatalogOptions) {
  const mode = toRef(options.mode)
  const category = toRef(options.category)
  const homePreferences: HomePreferences = useStorage().getHomePreferences()
  const sectionElementRefs = new Map<string, HTMLElement>()
  const loadingSectionKeys = shallowRef(new Set<string>())
  const sectionLoadErrors = shallowRef<Record<string, string>>({})
  let sectionObserver: IntersectionObserver | null = null

  const { catalog, isLoading, hasLoaded, errorMessage } = homeCatalogData({
    mode,
    category,
  })

  const pinnedSectionCollectionMode = computed<MediaCardCollectionMode>(
    () => homePreferences.favoriteCollectionMode.value,
  )
  const showSectionEditingButtons = computed(
    () => homePreferences.showSectionEditingButtons.value,
  )
  const currentCatalog = computed<HomeCatalogData>(
    () => catalog.value ?? { banners: [], categories: [], sections: [] },
  )
  const hasContent = computed(
    () =>
      currentCatalog.value.banners.length > 0 ||
      currentCatalog.value.categories.length > 0 ||
      currentCatalog.value.sections.length > 0,
  )
  const backgroundMediaItems = computed<BackgroundMediaCandidate[]>(() => {
    const items: BackgroundMediaCandidate[] = []
    const uniqueImageUrls = new Set<string>()

    currentCatalog.value.banners.forEach((banner) => {
      const imageUrl = banner.imageUrl?.trim() || null

      if (!imageUrl) {
        return
      }

      if (uniqueImageUrls.has(imageUrl)) {
        return
      }

      uniqueImageUrls.add(imageUrl)
      items.push({ imageUrl, videoUrl: null })
    })

    return items
  })
  const isHomeMode = computed(() => mode.value === 'home')
  const pinnableSections = computed(() =>
    currentCatalog.value.sections.filter((section) => isSectionPinnable(section)),
  )
  const pinnedSectionKeys = computed(() => {
    if (!isHomeMode.value) {
      return []
    }

    const visiblePinnableSectionKeys = new Set(
      pinnableSections.value.map((section) => section.preferenceKey),
    )

    return homePreferences.pinnedSectionOrder.value.filter((key) =>
      visiblePinnableSectionKeys.has(key),
    )
  })
  const pinnedSections = computed<HomeSection[]>(() => {
    if (!isHomeMode.value) {
      return []
    }

    const sectionsByPreferenceKey = new Map(
      pinnableSections.value.map((section) => [section.preferenceKey, section] as const),
    )

    return pinnedSectionKeys.value.flatMap((preferenceKey) => {
      const section = sectionsByPreferenceKey.get(preferenceKey)
      return section ? [section] : []
    })
  })
  const pinnedSectionKeySet = computed(() => new Set(pinnedSectionKeys.value))
  const otherSections = computed(() =>
    currentCatalog.value.sections.filter(
      (section) => !pinnedSectionKeySet.value.has(section.preferenceKey),
    ),
  )

  watch(
    backgroundMediaItems,
    (mediaItems) => {
      options.onBackgroundMediaItemsChange?.(mediaItems)
    },
    { immediate: true },
  )

  /**
   * Returns the thumbnail orientation override for one section.
   *
   * @param section Section displayed in the current catalog.
   */
  function getSectionThumbnailOrientation(section: HomeSection): ThumbnailOrientation | undefined {
    return homePreferences.sectionThumbnailOrientation.value[section.preferenceKey]
  }

  /**
   * Returns the thumbnail image fit override for one section.
   *
   * @param section Section displayed in the current catalog.
   */
  function getSectionThumbnailImageFit(section: HomeSection): ThumbnailImageFit | undefined {
    return homePreferences.sectionThumbnailImageFit.value[section.preferenceKey]
  }

  /**
   * Registers a rendered section root for deferred section loading.
   *
   * @param section Section represented by the rendered element.
   * @param element Rendered section root or null when Vue unmounts it.
   */
  function setSectionElementRef(section: HomeSection, element: Element | null) {
    const key = section.preferenceKey

    if (!(element instanceof HTMLElement)) {
      sectionElementRefs.delete(key)
      return
    }

    element.dataset.sectionKey = key
    sectionElementRefs.set(key, element)

    if (sectionObserver && shouldLoadInitialSection(section)) {
      sectionObserver.observe(element)
    }
  }

  /**
   * Returns whether one section is currently requesting backend entries.
   *
   * @param section Section displayed in the current catalog.
   */
  function isSectionLoading(section: HomeSection): boolean {
    return loadingSectionKeys.value.has(section.preferenceKey)
  }

  /**
   * Returns whether one section can request another page from the backend.
   *
   * @param section Section displayed in the current catalog.
   */
  function canLoadMoreSection(section: HomeSection): boolean {
    return section.sources.length > 0 && section.haveMore && !isSectionLoading(section)
  }

  /**
   * Returns the last deferred loading error for one section.
   *
   * @param section Section displayed in the current catalog.
   */
  function getSectionLoadError(section: HomeSection): string | null {
    return sectionLoadErrors.value[section.preferenceKey] || null
  }

  /**
   * Requests the next backend page for one section.
   *
   * @param section Section displayed in the current catalog.
   */
  function handleLoadMoreSection(section: HomeSection) {
    if (!canLoadMoreSection(section)) {
      return
    }

    void loadSectionPage(section, section.currentPage + 1)
  }

  /**
   * Returns whether one section exposes a visible label and can be pinned.
   *
   * @param section Section displayed in the current catalog.
   */
  function isSectionPinnable(section: HomeSection): boolean {
    return mode.value === 'home' && Boolean(section.label)
  }

  /**
   * Returns whether one section is currently pinned in the visible home order.
   *
   * @param section Section displayed in the current catalog.
   */
  function isSectionPinned(section: HomeSection): boolean {
    return pinnedSectionKeySet.value.has(section.preferenceKey)
  }

  /**
   * Returns whether one pinned section can be moved within the visible pinned subset.
   *
   * @param section Section displayed in the current catalog.
   * @param direction Direction requested by the UI.
   */
  function canMovePinnedSection(section: HomeSection, direction: 'up' | 'down'): boolean {
    const currentIndex = pinnedSectionKeys.value.indexOf(section.preferenceKey)
    return currentIndex >= 0 && (
      direction === 'up'
        ? currentIndex > 0
        : currentIndex < pinnedSectionKeys.value.length - 1
    )
  }

  /**
   * Toggles the pinned state of one section.
   *
   * @param section Section selected from the home catalog.
   */
  function toggleSectionPinned(section: HomeSection) {
    if (!isSectionPinnable(section)) {
      return
    }

    if (isSectionPinned(section)) {
      homePreferences.pinnedSectionOrder.value = homePreferences.pinnedSectionOrder.value.filter(
        (preferenceKey) => preferenceKey !== section.preferenceKey,
      )
      removeSectionDisplayPreferences(section.preferenceKey)
      return
    }

    homePreferences.pinnedSectionOrder.value = [
      ...homePreferences.pinnedSectionOrder.value,
      section.preferenceKey,
    ]
  }

  /**
   * Moves one pinned section inside the visible pinned subset.
   *
   * @param section Section selected from the home catalog.
   * @param direction Direction requested by the user.
   */
  function movePinnedSection(section: HomeSection, direction: 'up' | 'down') {
    const currentVisibleIndex = pinnedSectionKeys.value.indexOf(section.preferenceKey)
    const targetVisibleIndex = direction === 'up' ? currentVisibleIndex - 1 : currentVisibleIndex + 1
    const targetPreferenceKey = pinnedSectionKeys.value[targetVisibleIndex]

    if (currentVisibleIndex < 0 || !targetPreferenceKey) {
      return
    }

    const nextOrder = [...homePreferences.pinnedSectionOrder.value]
    const currentOrderIndex = nextOrder.indexOf(section.preferenceKey)
    const targetOrderIndex = nextOrder.indexOf(targetPreferenceKey)

    if (currentOrderIndex < 0 || targetOrderIndex < 0) {
      return
    }

    nextOrder[currentOrderIndex] = targetPreferenceKey
    nextOrder[targetOrderIndex] = section.preferenceKey
    homePreferences.pinnedSectionOrder.value = nextOrder
  }

  /**
   * Updates the thumbnail orientation for a specific section.
   *
   * @param sectionPreferenceKey The preference key of the section.
   * @param orientation The new orientation value.
   */
  function updateSectionThumbnailOrientation(
    sectionPreferenceKey: string,
    orientation: ThumbnailOrientation | null,
  ) {
    if (!orientation) {
      return
    }

    homePreferences.sectionThumbnailOrientation.value = {
      ...homePreferences.sectionThumbnailOrientation.value,
      [sectionPreferenceKey]: orientation,
    }
  }

  /**
   * Updates the thumbnail image fit for a specific section.
   *
   * @param sectionPreferenceKey The preference key of the section.
   * @param imageFit The new image fit value.
   */
  function updateSectionThumbnailImageFit(
    sectionPreferenceKey: string,
    imageFit: ThumbnailImageFit | null,
  ) {
    if (!imageFit) {
      return
    }

    homePreferences.sectionThumbnailImageFit.value = {
      ...homePreferences.sectionThumbnailImageFit.value,
      [sectionPreferenceKey]: imageFit,
    }
  }

  /**
   * Returns whether one section still needs its first deferred backend page.
   *
   * @param section Section displayed in the current catalog.
   */
  function shouldLoadInitialSection(section: HomeSection): boolean {
    return section.sources.length > 0 && section.items.length === 0
  }

  /**
   * Loads the requested deferred page and replaces the section in the current catalog.
   *
   * @param section Section displayed in the current catalog.
   * @param page 1-based page requested from the backend.
   */
  async function loadSectionPage(section: HomeSection, page: number) {
    if (section.sources.length === 0 || isSectionLoading(section)) {
      return
    }

    const key = section.preferenceKey
    loadingSectionKeys.value = new Set([...loadingSectionKeys.value, key])
    sectionLoadErrors.value = { ...sectionLoadErrors.value, [key]: '' }

    try {
      const nextSection = await loadHomeSectionPage(section, page)
      replaceCatalogSection(nextSection)
    } catch (error) {
      sectionLoadErrors.value = {
        ...sectionLoadErrors.value,
        [key]: error instanceof Error ? error.message : t('errors.loadingSectionFailed'),
      }
    } finally {
      const nextLoadingKeys = new Set(loadingSectionKeys.value)
      nextLoadingKeys.delete(key)
      loadingSectionKeys.value = nextLoadingKeys
    }
  }

  /**
   * Replaces one section while preserving the rest of the current catalog payload.
   *
   * @param nextSection Updated section returned by the backend normalizer.
   */
  function replaceCatalogSection(nextSection: HomeSection) {
    if (!catalog.value) {
      return
    }

    catalog.value = {
      ...catalog.value,
      sections: catalog.value.sections.map((section) =>
        section.preferenceKey === nextSection.preferenceKey ? nextSection : section,
      ),
    }
  }

  /**
   * Starts observing currently rendered deferred sections.
   */
  function observeDeferredSections() {
    if (!sectionObserver) {
      return
    }

    sectionObserver.disconnect()
    for (const [key, element] of sectionElementRefs) {
      const section = currentCatalog.value.sections.find((item) => item.preferenceKey === key)
      if (section && shouldLoadInitialSection(section)) {
        sectionObserver.observe(element)
      }
    }
  }

  /**
   * Removes persisted display overrides for one section.
   *
   * @param sectionPreferenceKey Preference key of the unpinned section.
   */
  function removeSectionDisplayPreferences(sectionPreferenceKey: string) {
    const { [sectionPreferenceKey]: _orientation, ...nextOrientations } =
      homePreferences.sectionThumbnailOrientation.value
    const { [sectionPreferenceKey]: _imageFit, ...nextImageFits } =
      homePreferences.sectionThumbnailImageFit.value

    homePreferences.sectionThumbnailOrientation.value = nextOrientations
    homePreferences.sectionThumbnailImageFit.value = nextImageFits
  }

  onMounted(() => {
    sectionObserver = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (!entry.isIntersecting) {
            return
          }

          const key = (entry.target as HTMLElement).dataset.sectionKey
          const section = currentCatalog.value.sections.find((item) => item.preferenceKey === key)
          if (!section || !shouldLoadInitialSection(section)) {
            sectionObserver?.unobserve(entry.target)
            return
          }

          sectionObserver?.unobserve(entry.target)
          void loadSectionPage(section, 1)
        })
      },
      { rootMargin: '320px 0px' },
    )

    void nextTick(observeDeferredSections)
  })

  onBeforeUnmount(() => {
    sectionObserver?.disconnect()
    sectionObserver = null
    sectionElementRefs.clear()
  })

  watch(
    () =>
      currentCatalog.value.sections
        .map((section) => `${section.preferenceKey}:${section.items.length}:${section.sources.length}`)
        .join('|'),
    () => {
      void nextTick(observeDeferredSections)
    },
  )

  return {
    catalog,
    isLoading,
    hasLoaded,
    errorMessage,
    currentCatalog,
    hasContent,
    isHomeMode,
    pinnedSectionCollectionMode,
    pinnedSections,
    otherSections,
    showSectionEditingButtons,
    getSectionThumbnailOrientation,
    getSectionThumbnailImageFit,
    setSectionElementRef,
    isSectionLoading,
    getSectionLoadError,
    handleLoadMoreSection,
    isSectionPinned,
    canMovePinnedSection,
    toggleSectionPinned,
    movePinnedSection,
    updateSectionThumbnailOrientation,
    updateSectionThumbnailImageFit,
  }
}
