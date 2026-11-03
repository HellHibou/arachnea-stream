<script setup lang="ts">
import { computed, shallowRef, toRef } from 'vue'

import HomeCategoryStrip from '@/components/home/HomeCategoryStrip.vue'
import HomeHeroBanner from '@/components/home/HomeHeroBanner.vue'
import HomeSectionActions from '@/components/home/HomeSectionActions.vue'
import HomeSectionCollectionToggle from '@/components/home/HomeSectionCollectionToggle.vue'
import CustomHomeManager from '@/components/home/CustomHomeManager.vue'
import MediaCardCollection from '@/components/MediaCardCollection.vue'
import ScrollToTopButton from '@/components/ScrollToTopButton.vue'
import { useHomeCatalog } from '@/composables/home/useHomeCatalog'
import { useScrollToTop } from '@/composables/useScrollToTop'
import { useI18n } from '@/i18n'
import { useStorage } from '@/services/storage'
import type { HomeCategory, HomeSection, HomeSectionSubsection } from '@/types/home'
import type { CustomHomeDefinition, CustomHomeAddIntent } from '@/types/customHome'
import type {
  MediaCardCollectionMode,
  CatalogItem,
  MediaItem,
  MediaSelectionTarget,
  BackgroundMediaCandidate,
  ThumbnailImageFit,
  ThumbnailOrientation,
} from '@/types/media'

/** Storage service instance. */
const storage = useStorage()

/**
 * Updates one section collection mode while preserving its current thumbnail options.
 *
 * @param section Section whose collection mode should be updated.
 * @param mode New collection mode, or null to ignore the update.
 */
function updateSectionCollectionMode(section: HomeSection, mode: MediaCardCollectionMode | null): void {
  if (!mode) {
    return
  }

  clearSectionCollectionModeOverride(section.preferenceKey)

  void storage.updateSectionCollectionMode(section.preferenceKey, mode, {
    thumbnailOrientation: getSectionThumbnailOrientation(section) ?? props.thumbnailOrientation,
    thumbnailImageFit: getSectionThumbnailImageFit(section) ?? props.thumbnailImageFit,
  })
}

/**
 * Removes the ephemeral collection mode override applied by the section toggle.
 *
 * @param sectionPreferenceKey Preference key of the section to reset.
 */
function clearSectionCollectionModeOverride(sectionPreferenceKey: string): void {
  if (!(sectionPreferenceKey in sectionCollectionModeOverrides.value)) {
    return
  }

  const nextOverrides = { ...sectionCollectionModeOverrides.value }
  delete nextOverrides[sectionPreferenceKey]
  sectionCollectionModeOverrides.value = nextOverrides
}

/**
 * Updates one section thumbnail orientation while preserving its other display options.
 *
 * @param section Section whose thumbnail orientation should be updated.
 * @param orientation New orientation, or null to ignore the update.
 */
function updateSectionThumbnailOrientation(section: HomeSection, orientation: ThumbnailOrientation | null): void {
  if (!orientation) {
    return
  }

  void storage.updateSectionThumbnailOrientation(section.preferenceKey, orientation, {
    collectionMode: getSectionCollectionMode(section),
    thumbnailImageFit: getSectionThumbnailImageFit(section) ?? props.thumbnailImageFit,
  })
}

/**
 * Updates one section thumbnail image fit while preserving its other display options.
 *
 * @param section Section whose thumbnail image fit should be updated.
 * @param imageFit New image fit, or null to ignore the update.
 */
function updateSectionThumbnailImageFit(section: HomeSection, imageFit: ThumbnailImageFit | null): void {
  if (!imageFit) {
    return
  }

  void storage.updateSectionThumbnailImageFit(section.preferenceKey, imageFit, {
    collectionMode: getSectionCollectionMode(section),
    thumbnailOrientation: getSectionThumbnailOrientation(section) ?? props.thumbnailOrientation,
  })
}

/**
 * Props accepted by the home and category catalog screen.
 */
interface Props {
  /** Exact backend source id; null keeps the aggregated home. @default null */
  sourceId?: string | null
  /**
   * Active catalog mode.
   * @default 'home'
   */
  mode?: 'home' | 'category' | 'custom'
  /** Validated persisted definition or preview draft in custom mode. @default null */
  customDefinition?: CustomHomeDefinition | null
  /** Read-only draft preview disables edition controls and custom additions. @default false */
  previewOnly?: boolean
  /**
   * Selected category shown when the catalog runs in category mode.
   * @default null
   */
  category?: HomeCategory | null
  /**
   * Thumbnail orientation forwarded to every media collection.
   * @default 'portrait'
   */
  thumbnailOrientation?: ThumbnailOrientation
  /**
   * Thumbnail fit mode forwarded to every media collection.
   * @default 'cover'
   */
  thumbnailImageFit?: ThumbnailImageFit
  /**
   * Size multiplier applied to media card thumbnails.
   * @default 1
   */
  thumbnailSizeMultiplier?: number
}

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
  mode: 'home',
  customDefinition: null,
  previewOnly: false,
  category: null,
  sourceId: null,
  thumbnailOrientation: 'portrait',
  thumbnailImageFit: 'cover',
  thumbnailSizeMultiplier: 1,
})

const emit = defineEmits<{
  /** Emitted when a media item is selected. */
  'select-item': [target: MediaSelectionTarget]
  /** Emitted when a category is selected. */
  'select-category': [category: HomeCategory]
  /** Emitted when background media items should be updated. */
  'update:background-media-items': [mediaItems: BackgroundMediaCandidate[]]
}>()

/** Home catalog composable results. */
const {
  customIssues,
  retryCustomIssue,
  /** The catalog data. */
  catalog,
  /** Whether the catalog is currently loading. */
  isLoading,
  /** Whether the catalog has completed initial load. */
  hasLoaded,
  /** Error message from catalog loading. */
  errorMessage,
  /** Function to load deferred banners after the hero has mounted. */
  loadDeferredBanners,
  loadCatalog,
  bookmarkPreferenceKey,
  /** The current catalog being displayed. */
  currentCatalog,
  /** Whether there is content to display. */
  hasContent,
  /** Whether the current mode is home. */
  isHomeMode,
  /** List of pinned sections. */
  pinnedSections,
  /** List of non-pinned sections. */
  otherSections,
  /** Whether section editing buttons should be shown. */
  showSectionEditingButtons,
  /** Function to get thumbnail orientation for a section. */
  getSectionThumbnailOrientation,
  /** Function to get thumbnail image fit for a section. */
  getSectionThumbnailImageFit,
  /** Function to get collection mode for a section. */
  getSectionCollectionMode,
  /** Function to set section element reference. */
  setSectionElementRef,
  /** Function to check if a section is currently loading. */
  isSectionLoading,
  /** Function to get load error for a section. */
  getSectionLoadError,
  /** Function to handle loading more section items. */
  handleLoadMoreSection,
} = useHomeCatalog({
  mode: toRef(props, 'mode'),
  category: toRef(props, 'category'),
  sourceId: toRef(props, 'sourceId'),
  customDefinition: toRef(props, 'customDefinition'),
  onBackgroundMediaItemsChange: (mediaItems) => {
    emit('update:background-media-items', mediaItems)
  },
})

/**
 * Resolves the sticky toolbar height inherited by one catalog section.
 *
 * @param element Element inheriting the application toolbar height variable.
 * @returns Toolbar height in pixels, or 0 when the variable is unavailable.
 */
function getCatalogToolbarHeight(element: HTMLElement): number {
  const value = Number.parseFloat(
    getComputedStyle(element).getPropertyValue('--app-toolbar-height'),
  )

  return Number.isFinite(value) ? value : 0
}

/**
 * Resolves the catalog scroll-to-top destination.
 *
 * The button scrolls up to the nearest expanded (multi-row) section title.
 * When no expanded section starts above the viewport, the page scrolls to the
 * top instead.
 *
 * @returns Absolute vertical scroll offset in pixels.
 */
function resolveCatalogScrollTop(): number {
  const renderedSections = props.mode === 'category'
    ? renderedCatalogSections.value
    : [...renderedPinnedSections.value, ...renderedOtherSections.value]
  const expandedSectionKeys = new Set(
    renderedSections
      .filter(({ section }) => getActiveSectionCollectionMode(section) === 'grid')
      .map(({ section }) => section.preferenceKey),
  )
  const sectionElements = Array.from(document.querySelectorAll<HTMLElement>('[data-section-key]'))
  const toolbarHeight = sectionElements[0] ? getCatalogToolbarHeight(sectionElements[0]) : 0

  const targetTitle = sectionElements
    .filter((section) => expandedSectionKeys.has(section.dataset.sectionKey ?? ''))
    .reverse()
    .map((section) => section.querySelector<HTMLElement>('.media-card-collection__title'))
    .find((title) => title !== null && title.getBoundingClientRect().top < toolbarHeight)

  if (!targetTitle) {
    return 0
  }

  return Math.max(0, targetTitle.getBoundingClientRect().top + window.scrollY - toolbarHeight - 8)
}

/** Scroll-to-top button state and handler. */
const { showScrollToTop, scrollToTop } = useScrollToTop({
  resolveScrollTop: resolveCatalogScrollTop,
})
/** Internationalization utilities. */
const { t } = useI18n()
const addIntent = shallowRef<CustomHomeAddIntent | null>(null)
const addDialogOpen = shallowRef(false)
/** @param intent Explicit add action; never selects an implicit destination or writes storage. */
function requestAdd(intent: CustomHomeAddIntent): void { addIntent.value = intent; addDialogOpen.value = true }
const editingEnabled = computed(() => showSectionEditingButtons.value && !props.previewOnly && props.mode !== 'custom')
/** Active subsection index tracked independently for every parent section. */
const activeSubsectionIndexes = shallowRef<Record<string, number>>({})

interface RenderedHomeSection {
  section: HomeSection
  subsection: HomeSectionSubsection | null
  items: CatalogItem[]
  label: string | null
  haveMore: boolean
}

/**
 * Resolves the active selectable subsection for one parent section.
 *
 * @param section Parent section currently rendered in the catalog.
 * @returns Active subsection, or null for an ordinary section.
 */
function getActiveSubsection(section: HomeSection): HomeSectionSubsection | null {
  const subsections = section.subsections ?? []
  if (subsections.length === 0) {
    return null
  }

  const activeIndex = activeSubsectionIndexes.value[section.preferenceKey] ?? 0
  return subsections[((activeIndex % subsections.length) + subsections.length) % subsections.length] ?? null
}

/**
 * Builds the display model for a parent section and its active subsection.
 *
 * @param section Parent section currently rendered in the catalog.
 * @returns Data consumed by one visible media rail.
 */
function toRenderedHomeSection(section: HomeSection): RenderedHomeSection {
  const subsection = getActiveSubsection(section)
  return {
    section: props.mode === 'custom' && subsection ? {
      ...section, customSubsectionKey: subsection.key,
      haveMore: subsection.haveMore || subsection.sources.some((source) => !source.hasInitialItems || source.haveMore),
    } : section,
    subsection,
    items: subsection?.items ?? section.items,
    label: subsection ? [section.label, subsection.label].filter(Boolean).join(' — ') : section.label,
    haveMore: props.mode === 'custom' && subsection
      ? subsection.haveMore || subsection.sources.some((source) => !source.hasInitialItems || source.haveMore)
      : subsection?.haveMore ?? section.haveMore,
  }
}

/** Rendered home-mode section rails. */
const renderedPinnedSections = computed(() => pinnedSections.value.map(toRenderedHomeSection))
/** Rendered home-mode section rails that are not pinned. */
const renderedOtherSections = computed(() => otherSections.value.map(toRenderedHomeSection))
/** Rendered category-mode section rails. */
const renderedCatalogSections = computed(() => currentCatalog.value.sections.map(toRenderedHomeSection))
/**
 * Default collection mode of the category catalog: a single section keeps the
 * multi-row grid, while multiple sections each display as one compact row.
 */
const categoryDefaultCollectionMode = computed<MediaCardCollectionMode>(() =>
  renderedCatalogSections.value.length === 1 ? 'grid' : 'single-row',
)
/**
 * Ephemeral single-row/multi-row overrides applied by the section toggle.
 * These choices are never persisted as section preferences.
 */
const sectionCollectionModeOverrides = shallowRef<Record<string, MediaCardCollectionMode>>({})

/**
 * Resolves the collection mode rendered for one section in the active catalog.
 *
 * @param section Section displayed in the current catalog.
 * @returns Collection mode used by the section media rail.
 */
function getActiveSectionCollectionMode(section: HomeSection): MediaCardCollectionMode {
  const override = sectionCollectionModeOverrides.value[section.preferenceKey]

  if (override) {
    return override
  }

  return props.mode === 'category' ? categoryDefaultCollectionMode.value : getSectionCollectionMode(section)
}

/**
 * Returns whether a collection mode supports the single-row/multi-row toggle.
 *
 * @param collectionMode Collection mode rendered by the section.
 * @returns True for grid and single-row layouts.
 */
function canToggleSectionCollectionMode(collectionMode: MediaCardCollectionMode): boolean {
  return collectionMode === 'grid' || collectionMode === 'single-row'
}

/**
 * Switches one section between the single-row and multi-row layouts.
 *
 * The choice stays local to the current screen and is never written to section
 * preferences.
 *
 * @param section Section whose layout should be toggled.
 */
function toggleSectionCollectionMode(section: HomeSection): void {
  const collectionMode = getActiveSectionCollectionMode(section)

  if (!canToggleSectionCollectionMode(collectionMode)) {
    return
  }

  const nextMode: MediaCardCollectionMode = collectionMode === 'single-row' ? 'grid' : 'single-row'

  sectionCollectionModeOverrides.value = {
    ...sectionCollectionModeOverrides.value,
    [section.preferenceKey]: nextMode,
  }
}

/**
 * Cycles a parent section through its selectable subsections.
 *
 * @param section Parent section owning the subsections.
 * @param direction Direction in which to cycle.
 */
function cycleSubsection(section: HomeSection, direction: -1 | 1): void {
  const subsectionCount = section.subsections?.length ?? 0
  if (subsectionCount < 2) {
    return
  }

  const currentIndex = activeSubsectionIndexes.value[section.preferenceKey] ?? 0
  activeSubsectionIndexes.value = {
    ...activeSubsectionIndexes.value,
    [section.preferenceKey]: (currentIndex + direction + subsectionCount) % subsectionCount,
  }
}

/**
 * Emits the selected entry target so the parent can open the details screen.
 *
 * @param target Entry target selected from either a banner or a media card.
 */
function handleSelectItem(target: MediaSelectionTarget) {
  emit('select-item', target)
}

/**
 * Emits the selected category so the parent can load its aggregated catalog.
 *
 * @param category Category selected from the home/category strip.
 */
function handleSelectCategory(category: HomeCategory) {
  emit('select-category', category)
}

/**
 * Emits the selected media item from one collection rail.
 *
 * @param item Media item selected from the rendered catalog rails.
 */
function handleSelectCollectionItem(item: MediaItem) {
  emit('select-item', item)
}
</script>

<template>
  <main class="home-catalog">
    <section class="home-catalog__shell">
      <v-alert v-for="issue in customIssues" :key="issue.id" type="warning" variant="tonal" role="status">
        {{ issue.message }}
        <span v-if="issue.repairRequired">{{ t('customHome.repairRequired') }}</span>
        <v-btn @click="retryCustomIssue(issue.id)">{{ t('sourceHome.retry') }}</v-btn>
      </v-alert>
      <article v-if="errorMessage" class="home-catalog__empty">
        <h2 class="home-catalog__empty-title">{{ t('catalog.loadingErrorTitle') }}</h2>
        <p class="home-catalog__empty-copy">
          {{ errorMessage }}
        </p>
        <v-btn @click="loadCatalog">{{ t('sourceHome.retry') }}</v-btn>
      </article>

      <article v-else-if="isLoading && (!catalog || props.mode === 'category')" class="home-catalog__empty">
        <h2 class="home-catalog__empty-title">{{ t('entry.loadingTitle') }}</h2>
        <p class="home-catalog__empty-copy">
          {{ t('entry.loadingMessage') }}
        </p>
      </article>

      <article v-else-if="hasLoaded && !hasContent" class="home-catalog__empty">
        <h2 class="home-catalog__empty-title">{{ t('search.emptyTitle') }}</h2>
        <p class="home-catalog__empty-copy">
          {{ t('catalog.empty') }}
        </p>
        <v-btn v-if="props.sourceId !== null" @click="loadCatalog">{{ t('sourceHome.retry') }}</v-btn>
      </article>

      <template v-else>
        <section class="home-catalog__content">
          <HomeHeroBanner
            v-if="currentCatalog.banners.entries.length > 0 || currentCatalog.deferredBanners.length > 0"
            :banners="currentCatalog.banners.entries"
            :show-add-button="editingEnabled && props.mode !== 'custom'"
            @add-banners="(sourceId) => requestAdd({ kind: 'banners', sourceId })"
            :should-load-deferred-banners="currentCatalog.deferredBanners.some((collection) => Boolean(collection.link))"
            @select-item="handleSelectItem"
            @load-banners="loadDeferredBanners"
          />

          <header v-if="props.mode === 'category' && props.category" class="home-catalog__heading">
            <p class="home-catalog__eyebrow">{{ t('category.label') }}</p>
            <h1 class="home-catalog__title">{{ props.category.label }}</h1>
          </header>

          <section
            v-if="isHomeMode && renderedPinnedSections.length > 0"
            class="home-catalog__sections"
          >
            <div
              v-for="renderedSection in renderedPinnedSections"
              :key="renderedSection.section.id"
              :ref="(element) => setSectionElementRef(renderedSection.section, element as Element | null)"
              class="home-catalog__section-shell"
            >
              <MediaCardCollection
                :items="renderedSection.items"
                :label="renderedSection.label ?? undefined"
                :label-icon="renderedSection.section.preferenceKey === bookmarkPreferenceKey ? 'mdi-bookmark' : undefined"
                :mode="getActiveSectionCollectionMode(renderedSection.section)"
                :thumbnail-orientation="getSectionThumbnailOrientation(renderedSection.section) ?? props.thumbnailOrientation"
                :thumbnail-image-fit="getSectionThumbnailImageFit(renderedSection.section) ?? props.thumbnailImageFit"
                :thumbnail-size-multiplier="props.thumbnailSizeMultiplier"
                :show-header-actions="editingEnabled || Boolean(renderedSection.subsection)"
                :is-loading-more="isSectionLoading(renderedSection.section)"
                :have-more="renderedSection.haveMore"
                :load-more-error-message="getSectionLoadError(renderedSection.section)"
                :on-load-more="() => handleLoadMoreSection(renderedSection.section)"
                @select="handleSelectCollectionItem"
              >
                <template #header-leading>
                  <div class="home-catalog__section-leading">
                    <button
                      v-if="renderedSection.subsection"
                      class="home-catalog__subsection-navigation-button"
                      type="button"
                      :aria-label="t('catalog.previousSubsection')"
                      @click="cycleSubsection(renderedSection.section, -1)"
                    >
                      <v-icon icon="mdi-chevron-left" size="20" aria-hidden="true" />
                    </button>
                    <HomeSectionCollectionToggle
                      :mode="getActiveSectionCollectionMode(renderedSection.section)"
                      @toggle="toggleSectionCollectionMode(renderedSection.section)"
                    />
                  </div>
                </template>
                <template #header-actions>
                  <div
                    v-if="renderedSection.subsection"
                    class="home-catalog__subsection-navigation"
                    :aria-label="t('catalog.subsectionNavigation')"
                  >
                    <button
                      class="home-catalog__subsection-navigation-button"
                      type="button"
                      :aria-label="t('catalog.nextSubsection')"
                      @click="cycleSubsection(renderedSection.section, 1)"
                    >
                      <v-icon icon="mdi-chevron-right" size="20" aria-hidden="true" />
                    </button>
                  </div>
                  <HomeSectionActions
                    v-if="editingEnabled"
                    :show-add-button="props.mode !== 'custom'"
                    @add-section="(section) => requestAdd({ kind: 'section', section })"
                    :section="renderedSection.section"
                    :is-pinned="props.mode === 'custom'"
                    hide-pin-button
                    hide-move-buttons
                    :thumbnail-orientation="getSectionThumbnailOrientation(renderedSection.section) ?? props.thumbnailOrientation"
                    :thumbnail-image-fit="getSectionThumbnailImageFit(renderedSection.section) ?? props.thumbnailImageFit"
                    :collection-mode="getActiveSectionCollectionMode(renderedSection.section)"
                    @update-thumbnail-orientation="(_key, orientation) => updateSectionThumbnailOrientation(renderedSection.section, orientation)"
                    @update-thumbnail-image-fit="(_key, imageFit) => updateSectionThumbnailImageFit(renderedSection.section, imageFit)"
                    @update-collection-mode="(_key, mode) => updateSectionCollectionMode(renderedSection.section, mode)"
                  />
                </template>
              </MediaCardCollection>
            </div>
          </section>

          <HomeCategoryStrip
            :show-add-button="editingEnabled"
            @add-categories="(categories) => requestAdd({ kind: 'categories', categories })"
            v-if="currentCatalog.categories.length > 0"
            class="home-catalog__category-strip"
            :categories="currentCatalog.categories"
            @select-category="handleSelectCategory"
          />
        </section>

        <section v-if="currentCatalog.sections.length > 0" class="home-catalog__sections">
          <template v-if="isHomeMode">
            <div
              v-for="renderedSection in renderedOtherSections"
              :key="renderedSection.section.id"
              :ref="(element) => setSectionElementRef(renderedSection.section, element as Element | null)"
              class="home-catalog__section-shell"
            >
              <MediaCardCollection
                :items="renderedSection.items"
                :label="renderedSection.label ?? undefined"
                :mode="getActiveSectionCollectionMode(renderedSection.section)"
                :thumbnail-orientation="getSectionThumbnailOrientation(renderedSection.section) ?? props.thumbnailOrientation"
                :thumbnail-image-fit="getSectionThumbnailImageFit(renderedSection.section) ?? props.thumbnailImageFit"
                :thumbnail-size-multiplier="props.thumbnailSizeMultiplier"
                :show-header-actions="editingEnabled || Boolean(renderedSection.subsection)"
                :is-loading-more="isSectionLoading(renderedSection.section)"
                :have-more="renderedSection.haveMore"
                :load-more-error-message="getSectionLoadError(renderedSection.section)"
                :on-load-more="() => handleLoadMoreSection(renderedSection.section)"
                @select="handleSelectCollectionItem"
              >
                <template #header-leading>
                  <div class="home-catalog__section-leading">
                    <button
                      v-if="renderedSection.subsection"
                      class="home-catalog__subsection-navigation-button"
                      type="button"
                      :aria-label="t('catalog.previousSubsection')"
                      @click="cycleSubsection(renderedSection.section, -1)"
                    >
                      <v-icon icon="mdi-chevron-left" size="20" aria-hidden="true" />
                    </button>
                    <HomeSectionCollectionToggle
                      :mode="getActiveSectionCollectionMode(renderedSection.section)"
                      @toggle="toggleSectionCollectionMode(renderedSection.section)"
                    />
                  </div>
                </template>
                <template #header-actions>
                  <div
                    v-if="renderedSection.subsection"
                    class="home-catalog__subsection-navigation"
                    :aria-label="t('catalog.subsectionNavigation')"
                  >
                    <button
                      class="home-catalog__subsection-navigation-button"
                      type="button"
                      :aria-label="t('catalog.nextSubsection')"
                      @click="cycleSubsection(renderedSection.section, 1)"
                    >
                      <v-icon icon="mdi-chevron-right" size="20" aria-hidden="true" />
                    </button>
                  </div>
                  <HomeSectionActions
                    v-if="editingEnabled"
                    :show-add-button="props.mode !== 'custom'"
                    @add-section="(section) => requestAdd({ kind: 'section', section })"
                    :section="renderedSection.section"
                    :is-pinned="props.mode === 'custom'"
                    hide-pin-button
                    hide-move-buttons
                    :thumbnail-orientation="getSectionThumbnailOrientation(renderedSection.section) ?? props.thumbnailOrientation"
                    :thumbnail-image-fit="getSectionThumbnailImageFit(renderedSection.section) ?? props.thumbnailImageFit"
                    :collection-mode="getActiveSectionCollectionMode(renderedSection.section)"
                    @update-thumbnail-orientation="(_key, orientation) => updateSectionThumbnailOrientation(renderedSection.section, orientation)"
                    @update-thumbnail-image-fit="(_key, imageFit) => updateSectionThumbnailImageFit(renderedSection.section, imageFit)"
                    @update-collection-mode="(_key, mode) => updateSectionCollectionMode(renderedSection.section, mode)"
                  />
                </template>
              </MediaCardCollection>
            </div>
          </template>

          <template v-else>
            <div
              v-for="renderedSection in renderedCatalogSections"
              :key="renderedSection.section.id"
              :ref="(element) => setSectionElementRef(renderedSection.section, element as Element | null)"
              class="home-catalog__section-shell"
            >
              <MediaCardCollection
                :items="renderedSection.items"
                :label="renderedSection.label ?? undefined"
                :mode="getActiveSectionCollectionMode(renderedSection.section)"
                :thumbnail-orientation="props.thumbnailOrientation"
                :thumbnail-image-fit="props.thumbnailImageFit"
                :thumbnail-size-multiplier="props.thumbnailSizeMultiplier"
                :show-header-actions="Boolean(renderedSection.subsection)"
                :is-loading-more="isSectionLoading(renderedSection.section)"
                :have-more="renderedSection.haveMore"
                :load-more-error-message="getSectionLoadError(renderedSection.section)"
                :on-load-more="() => handleLoadMoreSection(renderedSection.section)"
                @select="handleSelectCollectionItem"
              >
                <template #header-leading>
                  <div class="home-catalog__section-leading">
                    <button
                      v-if="renderedSection.subsection"
                      class="home-catalog__subsection-navigation-button"
                      type="button"
                      :aria-label="t('catalog.previousSubsection')"
                      @click="cycleSubsection(renderedSection.section, -1)"
                    >
                      <v-icon icon="mdi-chevron-left" size="20" aria-hidden="true" />
                    </button>
                    <HomeSectionCollectionToggle
                      :mode="getActiveSectionCollectionMode(renderedSection.section)"
                      @toggle="toggleSectionCollectionMode(renderedSection.section)"
                    />
                  </div>
                </template>
                <template #header-actions>
                  <div
                    v-if="renderedSection.subsection"
                    class="home-catalog__subsection-navigation"
                    :aria-label="t('catalog.subsectionNavigation')"
                  >
                    <button
                      class="home-catalog__subsection-navigation-button"
                      type="button"
                      :aria-label="t('catalog.nextSubsection')"
                      @click="cycleSubsection(renderedSection.section, 1)"
                    >
                      <v-icon icon="mdi-chevron-right" size="20" aria-hidden="true" />
                    </button>
                  </div>
                </template>
              </MediaCardCollection>
            </div>
          </template>
        </section>
      </template>
    </section>

    <CustomHomeManager v-if="!props.previewOnly" v-model:open="addDialogOpen" :intent="addIntent" :show-trigger="false" />
    <ScrollToTopButton
      v-if="showScrollToTop"
      @click="scrollToTop"
    />
  </main>
</template>

<style scoped>
.home-catalog {
  position: relative;
  min-height: 100vh;
  min-height: 100dvh;
  overflow: hidden;
  background: var(--bg-transparent);
  color: var(--text-primary);
}

.home-catalog__shell {
  position: relative;
  z-index: 1;
  width: calc(100% - 32px);
  margin: 0 auto;
  padding: 16px 0 32px;
}

.home-catalog__content {
  display: grid;
  gap: 26px;
}

.home-catalog__heading {
  display: grid;
  gap: 8px;
  padding: 4px 2px 0;
}

.home-catalog__eyebrow {
  color: rgba(207, 220, 235, 0.76);
  font-size: 0.82rem;
  font-weight: 700;
  letter-spacing: 0.18em;
  text-transform: uppercase;
}

.home-catalog__title {
  margin: 0 0 12px;
  color: var(--text-primary);
  font-size: clamp(1.7rem, 1.36rem + 1.1vw, 2.5rem);
  font-weight: 800;
  line-height: 1.04;
}

.home-catalog__category-strip {
  margin-bottom: 40px;
}

.home-catalog__sections {
  display: grid;
  gap: 28px;
}

.home-catalog__section-shell {
  display: grid;
  gap: 12px;
}

.home-catalog__section-leading {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.home-catalog__subsection-navigation {
  display: flex;
  gap: 4px;
}

.home-catalog__subsection-navigation-button {
  display: grid;
  width: 32px;
  height: 32px;
  place-items: center;
  border: 1px solid var(--border-color-primary);
  border-radius: 50%;
  background: var(--bg-surface);
  color: var(--text-primary);
  cursor: pointer;
}

.home-catalog__subsection-navigation-button:hover,
.home-catalog__subsection-navigation-button:focus-visible {
  border-color: var(--color-primary);
  background: var(--bg-surface-strong);
}

.home-catalog__empty {
  width: fit-content;
  max-width: min(100%, 620px);
  margin: 0 auto;
  padding: 36px 24px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  box-shadow: var(--inset-light);
  color: var(--text-secondary);
  text-align: center;
}

.home-catalog__empty-title {
  margin-bottom: 8px;
  font-size: 1.4rem;
}

.home-catalog__empty-copy {
  color: var(--text-secondary);
}

@media (max-width: 960px) {
  .home-catalog__shell {
    width: min(100%, calc(100% - 20px));
    padding: 12px 0 24px;
  }

  .home-catalog__content {
    gap: 22px;
  }
}
</style>