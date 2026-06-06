<script setup lang="ts">
import { toRef } from 'vue'

import HomeCategoryStrip from '@/components/home/HomeCategoryStrip.vue'
import HomeHeroBanner from '@/components/home/HomeHeroBanner.vue'
import HomeSectionActions from '@/components/home/HomeSectionActions.vue'
import MediaCardCollection from '@/components/MediaCardCollection.vue'
import ScrollToTopButton from '@/components/ScrollToTopButton.vue'
import { useHomeCatalog } from '@/composables/home/useHomeCatalog'
import { useScrollToTop } from '@/composables/useScrollToTop'
import { useI18n } from '@/i18n'
import type { HomeCategory } from '@/types/home'
import type {
  MediaCardCollectionMode,
  MediaItem,
  MediaSelectionTarget,
  BackgroundMediaCandidate,
  ThumbnailImageFit,
  ThumbnailOrientation,
} from '@/types/media'

const homeCollectionMode: MediaCardCollectionMode = 'single-row'
const categoryCollectionMode: MediaCardCollectionMode = 'grid'

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
}

const props = withDefaults(defineProps<Props>(), {
  mode: 'home',
  category: null,
  thumbnailOrientation: 'portrait',
  thumbnailImageFit: 'cover',
})

const emit = defineEmits<{
  'select-item': [target: MediaSelectionTarget]
  'select-category': [category: HomeCategory]
  'update:background-media-items': [mediaItems: BackgroundMediaCandidate[]]
}>()

const {
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
} = useHomeCatalog({
  mode: toRef(props, 'mode'),
  category: toRef(props, 'category'),
  onBackgroundMediaItemsChange: (mediaItems) => {
    emit('update:background-media-items', mediaItems)
  },
})

const { showScrollToTop, scrollToTop } = useScrollToTop()
const { t } = useI18n()

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
            v-if="currentCatalog.banners.length > 0"
            :banners="currentCatalog.banners"
            @select-item="handleSelectItem"
          />

          <header v-if="props.mode === 'category' && props.category" class="home-catalog__heading">
            <p class="home-catalog__eyebrow">{{ t('category.label') }}</p>
            <h1 class="home-catalog__title">{{ props.category.label }}</h1>
            <p v-if="props.category.description" class="home-catalog__description">
              {{ props.category.description }}
            </p>
          </header>

          <section
            v-if="isHomeMode && pinnedSections.length > 0"
            class="home-catalog__sections"
          >
            <div
              v-for="section in pinnedSections"
              :key="section.id"
              :ref="(element) => setSectionElementRef(section, element as Element | null)"
              class="home-catalog__section-shell"
            >
              <MediaCardCollection
                :items="section.items"
                :label="section.label ?? undefined"
                :mode="pinnedSectionCollectionMode"
                :thumbnail-orientation="getSectionThumbnailOrientation(section) ?? props.thumbnailOrientation"
                :thumbnail-image-fit="getSectionThumbnailImageFit(section) ?? props.thumbnailImageFit"
                :show-header-actions="showSectionEditingButtons"
                :is-loading-more="isSectionLoading(section)"
                :have-more="section.haveMore"
                :load-more-error-message="getSectionLoadError(section)"
                :on-load-more="() => handleLoadMoreSection(section)"
                @select="handleSelectCollectionItem"
              >
                <template #header-actions>
                  <HomeSectionActions
                    v-if="showSectionEditingButtons"
                    :section="section"
                    :is-pinned="isSectionPinned(section)"
                    :can-move-up="canMovePinnedSection(section, 'up')"
                    :can-move-down="canMovePinnedSection(section, 'down')"
                    :thumbnail-orientation="getSectionThumbnailOrientation(section) ?? props.thumbnailOrientation"
                    :thumbnail-image-fit="getSectionThumbnailImageFit(section) ?? props.thumbnailImageFit"
                    @toggle-pinned="toggleSectionPinned"
                    @move-pinned="movePinnedSection"
                    @update-thumbnail-orientation="updateSectionThumbnailOrientation"
                    @update-thumbnail-image-fit="updateSectionThumbnailImageFit"
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
              v-for="section in otherSections"
              :key="section.id"
              :ref="(element) => setSectionElementRef(section, element as Element | null)"
              class="home-catalog__section-shell"
            >
              <MediaCardCollection
                :items="section.items"
                :label="section.label ?? undefined"
                :mode="homeCollectionMode"
                :thumbnail-orientation="getSectionThumbnailOrientation(section) ?? props.thumbnailOrientation"
                :thumbnail-image-fit="getSectionThumbnailImageFit(section) ?? props.thumbnailImageFit"
                :show-header-actions="showSectionEditingButtons"
                :is-loading-more="isSectionLoading(section)"
                :have-more="section.haveMore"
                :load-more-error-message="getSectionLoadError(section)"
                :on-load-more="() => handleLoadMoreSection(section)"
                @select="handleSelectCollectionItem"
              >
                <template #header-actions>
                  <HomeSectionActions
                    v-if="showSectionEditingButtons"
                    :section="section"
                    :is-pinned="isSectionPinned(section)"
                    :can-move-up="canMovePinnedSection(section, 'up')"
                    :can-move-down="canMovePinnedSection(section, 'down')"
                    :thumbnail-orientation="getSectionThumbnailOrientation(section) ?? props.thumbnailOrientation"
                    :thumbnail-image-fit="getSectionThumbnailImageFit(section) ?? props.thumbnailImageFit"
                    @toggle-pinned="toggleSectionPinned"
                    @move-pinned="movePinnedSection"
                    @update-thumbnail-orientation="updateSectionThumbnailOrientation"
                    @update-thumbnail-image-fit="updateSectionThumbnailImageFit"
                  />
                </template>
              </MediaCardCollection>
            </div>
          </template>

          <template v-else>
            <div
              v-for="section in currentCatalog.sections"
              :key="section.id"
              :ref="(element) => setSectionElementRef(section, element as Element | null)"
              class="home-catalog__section-shell"
            >
              <MediaCardCollection
                :items="section.items"
                :label="section.label ?? undefined"
                :mode="categoryCollectionMode"
                :thumbnail-orientation="props.thumbnailOrientation"
                :thumbnail-image-fit="props.thumbnailImageFit"
                :is-loading-more="isSectionLoading(section)"
                :have-more="section.haveMore"
                :load-more-error-message="getSectionLoadError(section)"
                :on-load-more="() => handleLoadMoreSection(section)"
                @select="handleSelectCollectionItem"
              />
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

.home-catalog__description {
  max-width: 62ch;
  color: var(--text-secondary);
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
