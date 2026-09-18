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

import { loadHomeSectionPage, getStream } from '@/services/rustify'
import {
  BOOKMARKS_SECTION_PREFERENCE_KEY,
  type EntryBookmarkRecord,
} from '@/services/entryBookmarks'
import { useStorage, type HomePreferences } from '@/services/storage'
import {
  resolveBackendStreamMediaSource,
  resolveBackgroundMediaSource,
  resolveIframeMediaSource,
} from '@/services/players'
import type {
  HomeBanner,
  HomeCatalogData,
  HomeCategory,
  HomeSection,
} from '@/types/home'
import type { EntryPlayer } from '@/types/entry'
import type {
  BackgroundMediaCandidate,
  MediaCardCollectionMode,
  MediaItem,
  ThumbnailImageFit,
  ThumbnailOrientation,
} from '@/types/media'
import { homeCatalogData } from '@/composables/home/homeCatalogData'
import { t } from '@/i18n'

/** Maximum number of background media candidates exposed to the background layer. */
const BACKGROUND_MEDIA_ITEMS_LIMIT = 10

/** Maximum number of banner trailer videos exposed to the background layer. */
const BACKGROUND_MEDIA_VIDEO_ITEMS_LIMIT = 5

/**
 * Returns the first available thumbnail image URL for one media item.
 * Prefers landscape images for the full-page background, then falls back to the generic and poster URLs.
 *
 * @param item Media item rendered in a catalog section.
 * @returns Trimmed image URL, or null when no image is available.
 */
function toBackgroundThumbnailImageUrl(item: MediaItem): string | null {
  return item.imageLandscapeUrl?.trim() || item.imageUrl?.trim() || item.imagePosterUrl?.trim() || null
}

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
   * Callback invoked when background media items change.
   * Receives the ordered background image candidates whenever the current catalog changes.
   *
   * @param mediaItems - Array of background media candidates from banners and section thumbnails.
   */
  onBackgroundMediaItemsChange?: (mediaItems: BackgroundMediaCandidate[]) => void
}

/**
 * Maps one bookmark record to the media card shape rendered by the bookmarks section.
 *
 * @param record Bookmark record persisted in IndexedDB.
 * @returns Media item compatible with the existing card collection.
 */
function toBookmarkMediaItem(record: EntryBookmarkRecord): MediaItem {
  const seasonEpisodeLabel = [
    record.seasonLabel,
    record.episodeNumber !== null ? `E${record.episodeNumber}` : null,
  ]
    .filter((part) => Boolean(part))
    .join(' · ') || null

  return {
    id: record.key,
    title: record.title,
    alternativeTitleLabel: record.alternativeTitleLabel,
    imagePosterUrl: record.imagePosterUrl,
    imagePortraitUrl: null,
    imageLandscapeUrl: record.imageLandscapeUrl,
    imageUrl: record.imageUrl,
    source: record.source,
    entryUrl: record.entry,
    webUrl: null,
    mediaTypeLabel: record.mediaTypeLabel,
    mediaTypeValues: [],
    themeLabels: [],
    audioLabel: null,
    durationLabel: null,
    rating: null,
    overview: record.description,
    episodeLabel: seasonEpisodeLabel,
    releaseDateLabel: null,
    expireLabel: null,
    price: null,
    metaLine: record.isFullyWatched ? t('bookmarks.watchedBadge') : null,
  }
}

/**
 * Coordinates home catalog data, deferred section loading, and per-section preferences.
 *
 * @param options Reactive catalog mode, category, and optional background URL callback.
 * @returns State and actions consumed by the home catalog view.
 */
export function useHomeCatalog(options: UseHomeCatalogOptions) {
  /** Reactive reference to the catalog mode. */
  const mode = toRef(options.mode)
  /** Reactive reference to the selected category. */
  const category = toRef(options.category)
  /** Storage service instance. */
  const storage = useStorage()
  /** Application parameters from storage. */
  const parameters = storage.getParameters()
  /** Home preferences from storage. */
  const homePreferences: HomePreferences = storage.getHomePreferences()
  /** Map of section preference keys to their rendered DOM elements. */
  const sectionElementRefs = new Map<string, HTMLElement>()
  /** Set of section keys currently loading data. */
  const loadingSectionKeys = shallowRef(new Set<string>())
  /** Record of section load errors keyed by preference key. */
  const sectionLoadErrors = shallowRef<Record<string, string>>({})
  /** Intersection observer for lazy loading sections. */
  let sectionObserver: IntersectionObserver | null = null

  const { catalog, isLoading, hasLoaded, errorMessage, loadDeferredBanners } = homeCatalogData({
    mode,
    category,
  })

  /**
   * Returns the collection mode override for one section.
   *
   * @param section - Section displayed in the current catalog.
   * @returns The collection mode for this section.
   */
  function getSectionCollectionMode(section: HomeSection): MediaCardCollectionMode {
    return homePreferences.sectionCollectionMode.value[section.preferenceKey] ?? 'single-row'
  }
  /** Whether section editing buttons should be shown, from user preferences. */
  const showSectionEditingButtons = computed(
    () => homePreferences.showSectionEditingButtons.value,
  )
  /** The current catalog data with fallback empty object. */
  const currentCatalog = computed<HomeCatalogData>(
    () => catalog.value ?? { banners: { entries: [], source: '' }, deferredBanners: [], categories: [], sections: [] },
  )
  /** Whether the catalog has any content (banners, categories, or sections). */
  const hasContent = computed(
    () =>
      currentCatalog.value.banners.entries.length > 0 ||
      currentCatalog.value.categories.length > 0 ||
      currentCatalog.value.sections.length > 0,
  )
  /**
   * Resolved background video URLs by banner id for banners whose trailer
   * requires asynchronous player resolution through `get_stream`.
   */
  const bannerBackgroundVideoUrls = shallowRef<Record<string, string>>({})
  /** Banner ids whose background video resolution failed or returned nothing. */
  const failedBannerBackgroundVideoIds = new Set<string>()
  /** Request counter guarding against stale asynchronous banner resolutions. */
  let bannerVideoResolutionId = 0

  /**
   * Builds the minimal player descriptor required to resolve one banner stream.
   *
   * @param banner Banner containing an optional backend player resolver.
   * @returns Player descriptor, or null when the banner has no resolver.
   */
  function createBannerPlayer(banner: HomeBanner): EntryPlayer | null {
    if (!banner.player) {
      return null
    }

    return {
      id: `${banner.id}-background`,
      label: banner.title ?? banner.id,
      directLink: null,
      webLink: null,
      name: null,
      lang: null,
      resolver: banner.player,
      storyboard: null,
    }
  }

  /**
   * Resolves the playable video URL of one banner.
   *
   * Direct video URLs are used as-is while player-based banners call
   * `get_stream` on the backend, mirroring the hero banner behavior.
   *
   * @param banner Banner to resolve.
   * @returns Playable URL usable by the decorative background renderer, or null.
   */
  async function resolveBannerBackgroundVideoUrl(banner: HomeBanner): Promise<string | null> {
    const directSource = resolveBackgroundMediaSource(banner.videoUrl)

    if (directSource) {
      return directSource.src
    }

    const player = createBannerPlayer(banner)

    if (!player) {
      return null
    }

    const response = await getStream(player)

    if (!response) {
      return null
    }

    const source =
      'embedLink' in response
        ? resolveIframeMediaSource(response.embedLink)
        : resolveBackendStreamMediaSource(
            response.streamUrl[0] ?? null,
            response.manifestType,
            response.licenseUrl,
            response.licenseHeaders,
            response.storyboardVttUrl,
            response.chapters,
          )

    return source?.src ?? null
  }

  /**
   * Resolves pending banner trailers one by one until the background video
   * limit is reached or every candidate was attempted.
   *
   * Deferred `get_banners` arrivals re-trigger this loop through the watcher.
   */
  async function resolvePendingBannerBackgroundVideos(): Promise<void> {
    const requestId = ++bannerVideoResolutionId

    while (
      requestId === bannerVideoResolutionId &&
      parameters.useTrailerAsBackground.value
    ) {
      const knownIds = new Set([
        ...Object.keys(bannerBackgroundVideoUrls.value),
        ...failedBannerBackgroundVideoIds,
      ])
      let availableCount = 0
      let pendingBanner: HomeBanner | null = null

      for (const banner of currentCatalog.value.banners.entries) {
        if (!banner.videoUrl && !banner.player) {
          continue
        }

        if (banner.videoUrl || bannerBackgroundVideoUrls.value[banner.id]) {
          availableCount += 1
          continue
        }

        if (!knownIds.has(banner.id)) {
          pendingBanner = banner
          break
        }
      }

      if (availableCount >= BACKGROUND_MEDIA_VIDEO_ITEMS_LIMIT || !pendingBanner) {
        return
      }

      const banner = pendingBanner

      try {
        const url = await resolveBannerBackgroundVideoUrl(banner)

        if (requestId !== bannerVideoResolutionId) {
          return
        }

        if (url) {
          bannerBackgroundVideoUrls.value = {
            ...bannerBackgroundVideoUrls.value,
            [banner.id]: url,
          }
        } else {
          failedBannerBackgroundVideoIds.add(banner.id)
        }
      } catch {
        if (requestId === bannerVideoResolutionId) {
          failedBannerBackgroundVideoIds.add(banner.id)
        }
      }
    }
  }

  /** Restarts banner trailer resolution whenever banners or the parameter change. */
  watch(
    [
      () => parameters.useTrailerAsBackground.value,
      () => currentCatalog.value.banners.entries,
    ],
    () => {
      void resolvePendingBannerBackgroundVideos()
    },
    { immediate: true },
  )

  /**
   * Collects banner trailer video candidates in display order.
   *
   * Player-resolved trailers come from {@link bannerBackgroundVideoUrls}.
   * Each candidate keeps its paired banner image so the background layer can
   * fall back to it when the video is blocked by the security mode.
   *
   * @returns Array of banner video candidates, deduplicated by video URL and limited to
   * {@link BACKGROUND_MEDIA_VIDEO_ITEMS_LIMIT} entries.
   */
  function toBannerVideoBackgroundItems(): BackgroundMediaCandidate[] {
    const resolvedUrls = bannerBackgroundVideoUrls.value
    const items: BackgroundMediaCandidate[] = []
    const uniqueVideoUrls = new Set<string>()

    for (const banner of currentCatalog.value.banners.entries) {
      const videoUrl = banner.videoUrl?.trim() || resolvedUrls[banner.id]?.trim() || null

      if (!videoUrl || uniqueVideoUrls.has(videoUrl)) {
        continue
      }

      uniqueVideoUrls.add(videoUrl)
      items.push({ imageUrl: banner.imageUrl?.trim() || null, videoUrl })

      if (items.length >= BACKGROUND_MEDIA_VIDEO_ITEMS_LIMIT) {
        break
      }
    }

    return items
  }

  /**
   * Background media candidates derived from banners and section thumbnail images.
   *
   * When trailer usage is enabled and at least one banner exposes a playable
   * video, the banner videos (limited to five) replace the background images
   * entirely. Otherwise, banner images are completed with section thumbnail
   * images up to {@link BACKGROUND_MEDIA_ITEMS_LIMIT} entries.
   * Deduplicates image URLs for the background media component.
   *
   * @returns Array of background media candidates from banners and section thumbnails.
   */
  const backgroundMediaItems = computed<BackgroundMediaCandidate[]>(() => {
    if (parameters.useTrailerAsBackground.value) {
      const bannerVideoItems = toBannerVideoBackgroundItems()

      if (bannerVideoItems.length > 0) {
        return bannerVideoItems
      }
    }

    if (!parameters.useCatalogBannersAsBackground.value) {
      return []
    }

    const items: BackgroundMediaCandidate[] = []
    const uniqueImageUrls = new Set<string>()

    currentCatalog.value.banners.entries.forEach((banner) => {
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

    if (items.length >= BACKGROUND_MEDIA_ITEMS_LIMIT) {
      return items.slice(0, BACKGROUND_MEDIA_ITEMS_LIMIT)
    }

    for (const section of currentCatalog.value.sections) {
      for (const mediaItem of section.items) {
        if (items.length >= BACKGROUND_MEDIA_ITEMS_LIMIT) {
          return items
        }

        const imageUrl = toBackgroundThumbnailImageUrl(mediaItem)

        if (!imageUrl || uniqueImageUrls.has(imageUrl)) {
          continue
        }

        uniqueImageUrls.add(imageUrl)
        items.push({ imageUrl, videoUrl: null })
      }
    }

    return items
  })
  /** Whether the current mode is 'home' (as opposed to 'category'). */
  const isHomeMode = computed(() => mode.value === 'home')
  /**
   * Local bookmarks section built from the persisted bookmark records.
   *
   * Always considered pinned, hidden when empty or outside home mode. Items are
   * already sorted by last modification (most recent first) by the storage cache.
   */
  const bookmarksSection = computed<HomeSection | null>(() => {
    if (!isHomeMode.value || storage.entryBookmarks.length === 0) {
      return null
    }

    return {
      id: BOOKMARKS_SECTION_PREFERENCE_KEY,
      label: t('home.bookmarksSection'),
      preferenceKey: BOOKMARKS_SECTION_PREFERENCE_KEY,
      items: storage.entryBookmarks.map(toBookmarkMediaItem),
      sourceOrder: [],
      sources: [],
      currentPage: 1,
      haveMore: false,
    }
  })
  /** Sections that can be pinned, filtered from the current catalog. */
  const pinnableSections = computed(() =>
    currentCatalog.value.sections.filter((section) => isSectionPinnable(section)),
  )
  /**
   * Ordered list of pinned section preference keys that are currently visible.
   * Filters the user's pinned section order to only include visible, pinnable sections.
   *
   * @returns Array of preference keys for visible pinned sections in user's order.
   */
  const pinnedSectionKeys = computed(() => {
    if (!isHomeMode.value) {
      return []
    }

    const visiblePinnableSectionKeys = new Set(
      pinnableSections.value.map((section) => section.preferenceKey),
    )

    const orderedKeys = homePreferences.pinnedSectionOrder.value.filter(
      (key) =>
        visiblePinnableSectionKeys.has(key) ||
        key === BOOKMARKS_SECTION_PREFERENCE_KEY,
    )

    // The local bookmarks section is always pinned: it keeps its persisted
    // position when the user moved it, and defaults to the first position.
    if (bookmarksSection.value && !orderedKeys.includes(BOOKMARKS_SECTION_PREFERENCE_KEY)) {
      orderedKeys.unshift(BOOKMARKS_SECTION_PREFERENCE_KEY)
    }

    return orderedKeys
  })
  /**
   * Pinned sections in the order specified by user preferences.
   *
   * The local bookmarks section is injected according to its pinned position.
   *
   * @returns Array of pinned home sections.
   */
  const pinnedSections = computed<HomeSection[]>(() => {
    if (!isHomeMode.value) {
      return []
    }

    const sectionsByPreferenceKey = new Map(
      pinnableSections.value.map((section) => [section.preferenceKey, section] as const),
    )

    if (bookmarksSection.value) {
      sectionsByPreferenceKey.set(BOOKMARKS_SECTION_PREFERENCE_KEY, bookmarksSection.value)
    }

    return pinnedSectionKeys.value.flatMap((preferenceKey) => {
      const section = sectionsByPreferenceKey.get(preferenceKey)
      return section ? [section] : []
    })
  })
  /** Set of pinned section preference keys for quick lookup. */
  const pinnedSectionKeySet = computed(() => new Set(pinnedSectionKeys.value))
  /** Sections that are not pinned. */
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
   * @param section - Section displayed in the current catalog.
   * @returns The orientation override, or undefined if not set.
   */
  function getSectionThumbnailOrientation(section: HomeSection): ThumbnailOrientation | undefined {
    return homePreferences.sectionThumbnailOrientation.value[section.preferenceKey]
  }

  /**
   * Returns the thumbnail image fit override for one section.
   *
   * @param section - Section displayed in the current catalog.
   * @returns The image fit override, or undefined if not set.
   */
  function getSectionThumbnailImageFit(section: HomeSection): ThumbnailImageFit | undefined {
    return homePreferences.sectionThumbnailImageFit.value[section.preferenceKey]
  }

  /**
   * Registers a rendered section root for deferred section loading.
   *
   * @param section - Section represented by the rendered element.
   * @param element - Rendered section root or null when Vue unmounts it.
   */
  function setSectionElementRef(section: HomeSection, element: Element | null): void {
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
   * @param section - Section displayed in the current catalog.
   * @returns True if the section is currently loading.
   */
  function isSectionLoading(section: HomeSection): boolean {
    return loadingSectionKeys.value.has(section.preferenceKey)
  }

  /**
   * Returns whether one section can request another page from the backend.
   *
   * @param section - Section displayed in the current catalog.
   * @returns True if the section has sources, has more items, and is not currently loading.
   */
  function canLoadMoreSection(section: HomeSection): boolean {
    return section.sources.length > 0 && section.haveMore && !isSectionLoading(section)
  }

  /**
   * Returns the last deferred loading error for one section.
   *
   * @param section - Section displayed in the current catalog.
   * @returns The error message or null if no error.
   */
  function getSectionLoadError(section: HomeSection): string | null {
    return sectionLoadErrors.value[section.preferenceKey] || null
  }

  /**
   * Requests the next backend page for one section.
   *
   * @param section - Section displayed in the current catalog.
   */
  function handleLoadMoreSection(section: HomeSection): void {
    if (!canLoadMoreSection(section)) {
      return
    }

    void loadSectionPage(section, section.sources.filter((source) => source.haveMore))
  }

  /**
   * Returns whether one section exposes a visible label and can be pinned.
   *
   * @param section - Section displayed in the current catalog.
   * @returns True if the section can be pinned.
   */
  function isSectionPinnable(section: HomeSection): boolean {
    return mode.value === 'home' && Boolean(section.label)
  }

  /**
   * Returns whether one section is currently pinned in the visible home order.
   *
   * @param section - Section displayed in the current catalog.
   * @returns True if the section is pinned.
   */
  function isSectionPinned(section: HomeSection): boolean {
    return pinnedSectionKeySet.value.has(section.preferenceKey)
  }

  /**
   * Returns whether one pinned section can be moved within the visible pinned subset.
   *
   * @param section - Section displayed in the current catalog.
   * @param direction - Direction requested by the UI.
   * @returns True if the section can be moved in the specified direction.
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
   * @param section - Section selected from the home catalog.
   */
  function toggleSectionPinned(section: HomeSection): void {
    if (!isSectionPinnable(section)) {
      return
    }

    void storage.toggleSectionPinned(section.preferenceKey)
  }

  /**
   * Moves one pinned section inside the visible pinned subset.
   *
   * @param section - Section selected from the home catalog.
   * @param direction - Direction requested by the user.
   */
  function movePinnedSection(section: HomeSection, direction: 'up' | 'down'): void {
    void storage.movePinnedSection(section.preferenceKey, direction)
  }

  /**
   * Updates the thumbnail orientation for a specific section.
   *
   * @param sectionPreferenceKey - The preference key of the section.
   * @param orientation - The new orientation value.
   */
  function updateSectionThumbnailOrientation(
    sectionPreferenceKey: string,
    orientation: ThumbnailOrientation | null,
  ): void {
    if (!orientation) {
      return
    }

    void storage.updateSectionThumbnailOrientation(sectionPreferenceKey, orientation)
  }

  /**
   * Updates the thumbnail image fit for a specific section.
   *
   * @param sectionPreferenceKey - The preference key of the section.
   * @param imageFit - The new image fit value.
   */
  function updateSectionThumbnailImageFit(
    sectionPreferenceKey: string,
    imageFit: ThumbnailImageFit | null,
  ): void {
    if (!imageFit) {
      return
    }

    void storage.updateSectionThumbnailImageFit(sectionPreferenceKey, imageFit)
  }

  /**
   * Returns whether one section still needs its first deferred backend page.
   *
   * @param section - Section displayed in the current catalog.
   * @returns True if at least one source has not supplied its initial page.
   */
  function shouldLoadInitialSection(section: HomeSection): boolean {
    return section.sources.some((source) => !source.hasInitialItems)
  }

  /**
   * Loads the requested deferred page and replaces the section in the current catalog.
   *
   * @param section - Section displayed in the current catalog.
   * @param sources - Section sources to request from the backend.
   */
  async function loadSectionPage(section: HomeSection, sources: HomeSection['sources']): Promise<void> {
    if (sources.length === 0 || isSectionLoading(section)) {
      return
    }

    const key = section.preferenceKey
    loadingSectionKeys.value = new Set([...loadingSectionKeys.value, key])
    sectionLoadErrors.value = { ...sectionLoadErrors.value, [key]: '' }

    try {
      const nextSection = await loadHomeSectionPage(section, sources)
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
   * @param nextSection - Updated section returned by the backend normalizer.
   */
  function replaceCatalogSection(nextSection: HomeSection): void {
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
   * Sets up intersection observer for lazy loading.
   */
  function observeDeferredSections(): void {
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
          void loadSectionPage(section, section.sources.filter((source) => !source.hasInitialItems))
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
    loadDeferredBanners,
    currentCatalog,
    hasContent,
    isHomeMode,
    bookmarksSection,
    pinnedSections,
    otherSections,
    showSectionEditingButtons,
    getSectionThumbnailOrientation,
    getSectionThumbnailImageFit,
    getSectionCollectionMode,
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
