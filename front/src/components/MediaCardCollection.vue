<script setup lang="ts">
import { computed, toRef, useId, useSlots, useTemplateRef } from 'vue'

import type { MediaCardCollectionMode, MediaItem, ThumbnailImageFit, ThumbnailOrientation } from '@/types/media'

import MediaCard from './MediaCard.vue'
import { mediaCardCollectionPreviewManager } from '@/composables/media-card-collection/mediaCardCollectionPreviewManager'
import { mediaCardCollectionScroll } from '@/composables/media-card-collection/mediaCardCollectionScroll'
import { useI18n } from '@/i18n'

/**
 * Props accepted by the media card collection component.
 */
interface Props {
  /**
   * Items rendered by the collection.
   */
  items: MediaItem[]
  /**
   * Optional title displayed above the collection and exposed to assistive technology.
   */
  label?: string
  /**
   * Layout used to display the cards.
   * @default 'grid'
   */
  mode?: MediaCardCollectionMode
  /**
   * Thumbnail orientation applied to every media card.
   */
  thumbnailOrientation: ThumbnailOrientation
  /**
   * Poster fit mode applied to every media card.
   */
  thumbnailImageFit: ThumbnailImageFit
  /**
     * Hides list thumbnails when an item does not expose any poster.
     * @default false
     */
    hideMissingListThumbnails?: boolean
    /**
     * Whether to show the header actions slot.
     * @default true
     */
    showHeaderActions?: boolean
    /**
     * Indicates if the collection is loading more items.
     * @default false
     */
    isLoadingMore?: boolean
    /**
     * Indicates if more items are available to load.
     * @default false
     */
    haveMore?: boolean
    /**
     * Error message to display for load-more failures (null if no error).
     */
    loadMoreErrorMessage?: string | null
    /**
     * Callback invoked when the user requests more items.
     * Should be a no-op if isLoadingMore is true or haveMore is false.
     */
    onLoadMore?: () => void | Promise<void>
    /**
     * Label for the load-more button when idle.
     * @default 'Charger plus...'
     */
    loadMoreLabel?: string
    /**
     * Label shown while a load-more operation is in progress.
     * @default 'Chargement...'
     */
    loadingLabel?: string
    /**
     * Message displayed during initial loading when the collection is empty.
     * Useful for deferred sections.
     * @default 'Chargement de la section...'
     */
    initialLoadingMessage?: string
    /**
     * Indicates whether service logos should be displayed on card posters.
     * @default true
     */
    showServiceLogo?: boolean
  }

  const props = withDefaults(defineProps<Props>(), {
    mode: 'grid',
    hideMissingListThumbnails: false,
    showHeaderActions: true,
    isLoadingMore: false,
    haveMore: false,
    loadMoreErrorMessage: null,
    onLoadMore: undefined,
    showServiceLogo: true,
  })
const emit = defineEmits<{
  select: [item: MediaItem]
}>()
const { t } = useI18n()

const slots = useSlots()
const labelId = useId()
const viewportRef = useTemplateRef<HTMLDivElement>('viewport')
const itemsLength = computed(() => props.items.length)
const hasHeaderActions = computed(() => props.showHeaderActions && Boolean(slots['header-actions']))

const { canScrollLeft, canScrollRight, showScrollControls, updateScrollState, scrollRow } =
  mediaCardCollectionScroll({
    mode: toRef(props, 'mode'),
    itemsLength,
    thumbnailOrientation: toRef(props, 'thumbnailOrientation'),
    thumbnailImageFit: toRef(props, 'thumbnailImageFit'),
    viewportRef,
  })

const { openPreviewItemId, handlePreviewOpen, handlePreviewClose, handlePreviewRootChange } =
  mediaCardCollectionPreviewManager({
    mode: toRef(props, 'mode'),
  })

/**
 * Forwards the selected media item to the collection parent.
 *
 * @param item Media item requested from a child card.
 */
function handleSelect(item: MediaItem) {
  emit('select', item)
}

const effectiveLoadMoreLabel = computed(() => props.loadMoreLabel ?? t('catalog.loadMore'))
const effectiveLoadingLabel = computed(() => props.loadingLabel ?? t('catalog.loading'))
const effectiveInitialLoadingMessage = computed(() =>
  props.initialLoadingMessage ?? t('catalog.loadingSection'),
)
</script>

<template>
  <section
    class="media-card-collection-section"
    :class="{ 'media-card-collection-section--list': mode === 'list' }"
    :aria-label="label || t('catalog.ariaLabel')"
    :aria-labelledby="label ? labelId : undefined"
  >
    <header
      v-if="label || hasHeaderActions || showScrollControls"
      class="media-card-collection__header"
      :class="{ 'media-card-collection__header--controls-only': !label && !hasHeaderActions }"
    >
      <div v-if="label || hasHeaderActions" class="media-card-collection__heading">
        <h2 v-if="label" :id="labelId" class="media-card-collection__title">
          {{ label }}
        </h2>

        <div v-if="hasHeaderActions" class="media-card-collection__header-actions">
          <slot name="header-actions" />
        </div>
      </div>

      <!-- Show explicit navigation controls only when the one-line layout actually overflows horizontally. -->
      <div
        v-if="showScrollControls"
        class="media-card-collection__controls"
        :aria-label="t('catalog.horizontalNavigation')"
      >
        <button
          class="media-card-collection__control"
          type="button"
          :disabled="!canScrollLeft"
          :aria-label="t('catalog.scrollLeft')"
          @click="scrollRow('left')"
        >
          <v-icon icon="$NavigateBefore" size="24" aria-hidden="true" />
        </button>

        <button
          class="media-card-collection__control"
          type="button"
          :disabled="!canScrollRight"
          :aria-label="t('catalog.scrollRight')"
          @click="scrollRow('right')"
        >
          <v-icon icon="$NavigateNext" size="24" aria-hidden="true" />
        </button>
      </div>
    </header>

    <!-- Initial loading message for deferred sections -->
    <p
      v-if="isLoadingMore && items.length === 0 && effectiveInitialLoadingMessage"
      class="media-card-collection__initial-loading"
    >
      {{ effectiveInitialLoadingMessage }}
    </p>

    <!-- Keep horizontal scrolling on its own viewport so the section title stays fixed. -->
    <div
      ref="viewport"
      class="media-card-collection__viewport"
      :class="{ 'media-card-collection__viewport--single-row': mode === 'single-row' }"
      @scroll="updateScrollState"
    >
      <div
        class="media-card-collection"
        :class="[
          `media-card-collection--${thumbnailOrientation}`,
          `media-card-collection--${mode}`,
        ]"
      >
         <MediaCard
           v-for="item in items"
           :key="item.id"
           :item="item"
           :layout="mode === 'list' ? 'list' : 'card'"
           :thumbnail-orientation="thumbnailOrientation"
           :thumbnail-image-fit="thumbnailImageFit"
           :hide-missing-thumbnail="hideMissingListThumbnails"
           :is-preview-open="openPreviewItemId === item.id"
           :show-service-logo="showServiceLogo"
           @select="handleSelect"
           @preview-open="handlePreviewOpen"
           @preview-close="handlePreviewClose"
           @preview-root-change="handlePreviewRootChange"
         />

         <!-- Load-more button inline for single-row mode (scrollable area) -->
         <button
           v-if="(haveMore || isLoadingMore) && mode === 'single-row'"
           :class="[
             'media-card-collection__load-more',
             'media-card-collection__load-more-inline',
             isLoadingMore ? 'media-card-collection__load-more--loading' : 'media-card-collection__load-more--action'
           ]"
           type="button"
           :disabled="isLoadingMore || !haveMore"
           @click="onLoadMore"
         >
           {{ isLoadingMore ? effectiveLoadingLabel : effectiveLoadMoreLabel }}
         </button>
       </div>
     </div>

    <!-- Load-more error message -->
    <p
      v-if="loadMoreErrorMessage"
      class="media-card-collection__load-more-error"
    >
      {{ loadMoreErrorMessage }}
    </p>

    <!-- Load-more button (grid/list modes only) -->
    <button
      v-if="(haveMore || isLoadingMore) && mode !== 'single-row'"
      :class="[
        'media-card-collection__load-more',
        isLoadingMore ? 'media-card-collection__load-more--loading' : 'media-card-collection__load-more--action'
      ]"
      type="button"
      :disabled="isLoadingMore || !haveMore"
      @click="onLoadMore"
    >
      {{ isLoadingMore ? effectiveLoadingLabel : effectiveLoadMoreLabel }}
    </button>
  </section>
</template>

<style scoped>
.media-card-collection-section {
  display: grid;
  gap: 16px;
}

.media-card-collection-section--list {
  gap: 10px;
}

.media-card-collection__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.media-card-collection__header--controls-only {
  justify-content: flex-end;
}

.media-card-collection__heading {
  display: inline-flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}

.media-card-collection__title {
  margin: 0;
  min-width: 0;
  color: var(--text-primary);
  font-size: clamp(1.25rem, 1.12rem + 0.5vw, 1.6rem);
  font-weight: 700;
  letter-spacing: 0.01em;
}

.media-card-collection__header-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex: 0 0 auto;
}

.media-card-collection__controls {
  display: inline-flex;
  align-items: center;
  gap: 10px;
}

.media-card-collection__control {
  display: inline-grid;
  place-items: center;
  width: 42px;
  min-width: 42px;
  height: 42px;
  padding: 0;
  border: 1px solid var(--border-color-primary);
  border-radius: 50%;
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
  cursor: pointer;
  transition:
    transform var(--duration-fast) ease,
    border-color var(--duration-fast) ease,
    background-color var(--duration-fast) ease,
    color var(--duration-fast) ease;
}

.media-card-collection__control:hover:not(:disabled) {
  transform: translateY(-1px);
  border-color: var(--color-primary);
  background: var(--bg-surface);
}

.media-card-collection__control:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.media-card-collection__control:disabled {
  cursor: default;
  opacity: 0.42;
}

.media-card-collection__viewport {
  overflow: visible;
}

.media-card-collection__viewport--single-row {
  overflow-x: auto;
  padding: 4px 2px 12px;
  overscroll-behavior-x: contain;
  scroll-padding-inline: 2px;
  scroll-snap-type: x proximity;
  scrollbar-width: thin;
  scrollbar-color: var(--border-color-primary) transparent;
}

.media-card-collection__viewport--single-row::-webkit-scrollbar {
  height: 8px;
}

.media-card-collection__viewport--single-row::-webkit-scrollbar-thumb {
  border-radius: 999px;
  background: var(--border-color-primary);
}

.media-card-collection__viewport--single-row::-webkit-scrollbar-track {
  background: transparent;
}

.media-card-collection {
  --media-card-collection-min-column: 176px;
  --media-card-collection-row-width: 176px;
  --media-card-collection-row-gap: 18px;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(var(--media-card-collection-min-column), 1fr));
  gap: 22px 18px;
  align-items: start;
  overflow: visible;
}

.media-card-collection--landscape {
  /** Maximum width on landscape */
  --media-card-collection-min-column: 240px;
  --media-card-collection-row-width: 240px;
}

.media-card-collection--single-row {
  display: flex;
  width: 100%;
  gap: var(--media-card-collection-row-gap);
}

.media-card-collection--single-row > * {
  flex: 0 0 min(100%, var(--media-card-collection-row-width));
  min-width: 0;
  scroll-snap-align: start;
}

.media-card-collection--single-row > *:last-child {
  scroll-snap-align: end;
}

  .media-card-collection--list {
    grid-template-columns: minmax(0, 1fr);
    gap: 6px;
  }

  .media-card-collection--list > * {
    min-width: 0;
  }

  .media-card-collection__initial-loading {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.92rem;
    text-align: center;
    padding: 12px 0;
  }

  .media-card-collection__load-more-error {
    margin: 0;
    color: #ff9f9f;
    text-align: center;
    font-size: 0.92rem;
    padding: 8px 0;
  }

  .media-card-collection__load-more,
  .media-card-collection__load-more--loading,
  .media-card-collection__load-more--action {
    justify-self: center;
    min-height: 40px;
    padding: 0 18px;
    border: 1px solid var(--border-color-primary);
    border-radius: 999px;
    color: var(--text-primary);
    font: inherit;
    font-weight: 700;
    cursor: pointer;
    transition:
      transform var(--duration-fast) ease,
      border-color var(--duration-fast) ease,
      opacity var(--duration-fast) ease;
  }

  .media-card-collection__load-more--loading {
    background: var(--bg-surface);
    box-shadow: var(--inset-light);
  }

  .media-card-collection__load-more--action {
    background: var(--bg-accent-blue);
    box-shadow: var(--box-shadow-elevated);
  }

  .media-card-collection__load-more:hover:not(:disabled) {
    transform: translateY(-1px);
    border-color: var(--color-primary);
  }

  .media-card-collection__load-more:focus-visible {
    outline: var(--common-focus-ring);
    outline-offset: 2px;
  }

   .media-card-collection__load-more:disabled {
     cursor: default;
     opacity: 0.52;
   }

   /* Inline load-more button for single-row mode (inside scrollable area) */
   .media-card-collection--single-row .media-card-collection__load-more-inline {
     flex: 0 0 auto;
     align-self: center;
     margin-left: var(--media-card-collection-row-gap);
     scroll-snap-align: none;
   }

   @media (max-width: 640px) {
  .media-card-collection__header {
    flex-direction: column;
    align-items: stretch;
  }

  .media-card-collection__heading {
    width: 100%;
    justify-content: space-between;
    flex-wrap: wrap;
  }

  .media-card-collection__controls {
    justify-content: flex-end;
  }

  .media-card-collection__control {
    flex: 0 0 auto;
  }

  .media-card-collection {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 16px 14px;
  }

  .media-card-collection--landscape {
    grid-template-columns: 1fr;
  }

  .media-card-collection--single-row {
    --media-card-collection-row-gap: 14px;
  }

  .media-card-collection--single-row > * {
    flex-basis: min(78vw, var(--media-card-collection-row-width));
  }

  .media-card-collection--single-row.media-card-collection--landscape > * {
    flex-basis: min(92vw, var(--media-card-collection-row-width));
  }

  .media-card-collection--list {
    gap: 5px;
  }
}
</style>
