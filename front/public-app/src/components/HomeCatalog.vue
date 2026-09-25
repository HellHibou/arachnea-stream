<script setup lang="ts">
import { computed, shallowRef, toRef } from 'vue'

import HomeCategoryStrip from '@/components/home/HomeCategoryStrip.vue'
import HomeHeroBanner from '@/components/home/HomeHeroBanner.vue'
import HomeSectionActions from '@/components/home/HomeSectionActions.vue'
import MediaCardCollection from '@/components/MediaCardCollection.vue'
import ScrollToTopButton from '@/components/ScrollToTopButton.vue'
import { useHomeCatalog } from '@/composables/home/useHomeCatalog'
import { useScrollToTop } from '@/composables/useScrollToTop'
import { useI18n } from '@/i18n'
import { BOOKMARKS_SECTION_PREFERENCE_KEY } from '@/services/entryBookmarks'
import { useStorage } from '@/services/storage'
import type { HomeCategory, HomeSection, HomeSectionSubsection } from '@/types/home'
import type {
  MediaCardCollectionMode,
  MediaItem,
  MediaSelectionTarget,
  BackgroundMediaCandidate,
  ThumbnailImageFit,
  ThumbnailOrientation,
} from '@/types/media'

/** Collection mode for home page media card displays. */
const homeCollectionMode: MediaCardCollectionMode = 'single-row'
/** Collection mode for category page media card displays. */
const categoryCollectionMode: MediaCardCollectionMode = 'grid'
/** Storage service instance. */
const storage = useStorage()
/** Update section collection mode in preferences. */
function updateSectionCollectionMode(sectionPreferenceKey: string, mode: MediaCardCollectionMode | null): void {
  if (!mode) {
    return
  }

  void storage.updateSectionCollectionMode(sectionPreferenceKey, mode)
}

/**
 * Props accepted by the home and category catalog screen.
 */
interface Props {
  /**
   * Active catalog mode.
   * @default 'home'
   */
  mode?: 'home' | 'category'
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
  category: null,
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
  /** Function to check if a section is pinned. */
  isSectionPinned,
  /** Function to check if a pinned section can be moved. */
  canMovePinnedSection,
  /** Function to toggle section pinned state. */
  toggleSectionPinned,
  /** Function to move a pinned section. */
  movePinnedSection,
  /** Function to update section thumbnail orientation. */
  updateSectionThumbnailOrientation,
  /** Function to update section thumbnail image fit. */
  updateSectionThumbnailImageFit,
} = useHomeCatalog({
  mode: toRef(props, 'mode'),
  category: toRef(props, 'category'),
  onBackgroundMediaItemsChange: (mediaItems) => {
    emit('update:background-media-items', mediaItems)
  },
})

/** Scroll-to-top button state and handler. */
const { showScrollToTop, scrollToTop } = useScrollToTop()
/** Internationalization utilities. */
const { t } = useI18n()
/** Active subsection index tracked independently for every parent section. */
const activeSubsectionIndexes = shallowRef<Record<string, number>>({})

interface RenderedHomeSection {
  section: HomeSection
  subsection: HomeSectionSubsection | null
  items: MediaItem[]
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
    section,
    subsection,
    items: subsection?.items ?? section.items,
    label: subsection ? [section.label, subsection.label].filter(Boolean).join(' — ') : section.label,
    haveMore: subsection?.haveMore ?? section.haveMore,
  }
}

/** Rendered home-mode section rails. */
const renderedPinnedSections = computed(() => pinnedSections.value.map(toRenderedHomeSection))
/** Rendered home-mode section rails that are not pinned. */
const renderedOtherSections = computed(() => otherSections.value.map(toRenderedHomeSection))
/** Rendered category-mode section rails. */
const renderedCatalogSections = computed(() => currentCatalog.value.sections.map(toRenderedHomeSection))

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
      <article v-if="errorMessage" class="home-catalog__empty">
        <h2 class="home-catalog__empty-title">{{ t('catalog.loadingErrorTitle') }}</h2>
        <p class="home-catalog__empty-copy">
          {{ errorMessage }}
        </p>
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
      </article>

      <template v-else>
        <section class="home-catalog__content">
          <HomeHeroBanner
            v-if="currentCatalog.banners.entries.length > 0 || currentCatalog.deferredBanners.length > 0"
            :banners="currentCatalog.banners.entries"
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
                :label-icon="renderedSection.section.preferenceKey === BOOKMARKS_SECTION_PREFERENCE_KEY ? 'mdi-bookmark' : undefined"
                :mode="getSectionCollectionMode(renderedSection.section)"
                :thumbnail-orientation="getSectionThumbnailOrientation(renderedSection.section) ?? props.thumbnailOrientation"
                :thumbnail-image-fit="getSectionThumbnailImageFit(renderedSection.section) ?? props.thumbnailImageFit"
                :thumbnail-size-multiplier="props.thumbnailSizeMultiplier"
                :show-header-actions="showSectionEditingButtons || Boolean(renderedSection.subsection)"
                :is-loading-more="isSectionLoading(renderedSection.section)"
                :have-more="renderedSection.haveMore"
                :load-more-error-message="getSectionLoadError(renderedSection.section)"
                :on-load-more="() => handleLoadMoreSection(renderedSection.section)"
                @select="handleSelectCollectionItem"
              >
                <template v-if="renderedSection.subsection" #header-leading>
                  <button
                    class="home-catalog__subsection-navigation-button"
                    type="button"
                    :aria-label="t('catalog.previousSubsection')"
                    @click="cycleSubsection(renderedSection.section, -1)"
                  >
                    <v-icon icon="mdi-chevron-left" size="20" aria-hidden="true" />
                  </button>
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
                    v-if="showSectionEditingButtons"
                    :section="renderedSection.section"
                    :is-pinned="isSectionPinned(renderedSection.section)"
                    :hide-pin-button="renderedSection.section.preferenceKey === BOOKMARKS_SECTION_PREFERENCE_KEY"
                    :can-move-up="canMovePinnedSection(renderedSection.section, 'up')"
                    :can-move-down="canMovePinnedSection(renderedSection.section, 'down')"
                    :thumbnail-orientation="getSectionThumbnailOrientation(renderedSection.section) ?? props.thumbnailOrientation"
                    :thumbnail-image-fit="getSectionThumbnailImageFit(renderedSection.section) ?? props.thumbnailImageFit"
                    :collection-mode="getSectionCollectionMode(renderedSection.section)"
                    @toggle-pinned="toggleSectionPinned"
                    @move-pinned="movePinnedSection"
                    @update-thumbnail-orientation="updateSectionThumbnailOrientation"
                    @update-thumbnail-image-fit="updateSectionThumbnailImageFit"
                    @update-collection-mode="updateSectionCollectionMode"
                  />
                </template>
              </MediaCardCollection>
            </div>
          </section>

          <HomeCategoryStrip
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
                :mode="homeCollectionMode"
                :thumbnail-orientation="getSectionThumbnailOrientation(renderedSection.section) ?? props.thumbnailOrientation"
                :thumbnail-image-fit="getSectionThumbnailImageFit(renderedSection.section) ?? props.thumbnailImageFit"
                :thumbnail-size-multiplier="props.thumbnailSizeMultiplier"
                :show-header-actions="showSectionEditingButtons || Boolean(renderedSection.subsection)"
                :is-loading-more="isSectionLoading(renderedSection.section)"
                :have-more="renderedSection.haveMore"
                :load-more-error-message="getSectionLoadError(renderedSection.section)"
                :on-load-more="() => handleLoadMoreSection(renderedSection.section)"
                @select="handleSelectCollectionItem"
              >
                <template v-if="renderedSection.subsection" #header-leading>
                  <button
                    class="home-catalog__subsection-navigation-button"
                    type="button"
                    :aria-label="t('catalog.previousSubsection')"
                    @click="cycleSubsection(renderedSection.section, -1)"
                  >
                    <v-icon icon="mdi-chevron-left" size="20" aria-hidden="true" />
                  </button>
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
                    v-if="showSectionEditingButtons"
                    :section="renderedSection.section"
                    :is-pinned="isSectionPinned(renderedSection.section)"
                    :can-move-up="canMovePinnedSection(renderedSection.section, 'up')"
                    :can-move-down="canMovePinnedSection(renderedSection.section, 'down')"
                    :thumbnail-orientation="getSectionThumbnailOrientation(renderedSection.section) ?? props.thumbnailOrientation"
                    :thumbnail-image-fit="getSectionThumbnailImageFit(renderedSection.section) ?? props.thumbnailImageFit"
                    :collection-mode="getSectionCollectionMode(renderedSection.section)"
                    @toggle-pinned="toggleSectionPinned"
                    @move-pinned="movePinnedSection"
                    @update-thumbnail-orientation="updateSectionThumbnailOrientation"
                    @update-thumbnail-image-fit="updateSectionThumbnailImageFit"
                    @update-collection-mode="updateSectionCollectionMode"
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
                :mode="categoryCollectionMode"
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
                <template v-if="renderedSection.subsection" #header-leading>
                  <button
                    class="home-catalog__subsection-navigation-button"
                    type="button"
                    :aria-label="t('catalog.previousSubsection')"
                    @click="cycleSubsection(renderedSection.section, -1)"
                  >
                    <v-icon icon="mdi-chevron-left" size="20" aria-hidden="true" />
                  </button>
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