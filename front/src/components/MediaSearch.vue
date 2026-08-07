<script setup lang="ts">
import { toRef } from 'vue'

import type { MediaCardCollectionMode, MediaItem, ThumbnailImageFit, ThumbnailOrientation } from '@/types/media'

import MediaCardCollection from './MediaCardCollection.vue'
import ScrollToTopButton from '@/components/ScrollToTopButton.vue'
import { mediaSearchCollections } from '@/composables/media-search/mediaSearchCollections'
import { mediaSearchResults } from '@/composables/media-search/mediaSearchResults'
import { useScrollToTop } from '@/composables/useScrollToTop'
import { useI18n } from '@/i18n'

/** Available collection modes for the media search, excluding single-row. */
type SearchCollectionMode = Exclude<MediaCardCollectionMode, 'single-row'>

/**
 * Props accepted by the media search shell.
 */
interface Props {
   /**
    * Search query submitted by the parent controller.
    * @default ''
    */
   submittedQuery?: string
   /**
    * Monotonic key used to trigger a new search even when the submitted query did not change.
    * @default 0
    */
   searchRequestId?: number
   /**
    * Thumbnail orientation forwarded to the media card collection.
    * @default 'portrait'
    */
   thumbnailOrientation?: ThumbnailOrientation
   /**
    * Poster fit mode forwarded to the media card collection.
    * @default 'cover'
    */
   thumbnailImageFit?: ThumbnailImageFit
   /**
    * Layout mode used by the media card collection.
    * @default 'grid'
    */
   collectionMode?: SearchCollectionMode
   /**
    * Optional label displayed above the media card collection.
    */
   collectionLabel?: string
   /**
    * Selected backend media types used for the search request.
    * @default []
    */
   submittedMediaTypes?: string[]
   /**
    * Selected backend themes used for the search request.
    * @default []
    */
   submittedThemes?: string[]
}

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
   submittedQuery: '',
   searchRequestId: 0,
   thumbnailOrientation: 'portrait',
   thumbnailImageFit: 'cover',
   collectionMode: 'grid',
   submittedMediaTypes: () => [],
   submittedThemes: () => [],
})
const emit = defineEmits<{
  /** Emitted when a media item is selected. */
   'select-item': [item: MediaItem]
}>()

/** Search results state and actions. */
const {
   /** List of media items matching the search. */
   mediaItems,
   /** Whether a search has been performed. */
   hasSearched,
   /** Whether a search is currently in progress. */
   isSearching,
   /** Whether more items are currently loading. */
   isLoadingMore,
   /** Whether more items are available to load. */
   haveMore,
   /** Error message from search request. */
   errorMessage,
   /** Error message from load-more operation. */
   loadMoreErrorMessage,
   /** Function to load more search results. */
   loadMore,
} = mediaSearchResults({
   submittedQuery: toRef(props, 'submittedQuery'),
   searchRequestId: toRef(props, 'searchRequestId'),
   selectedMediaTypes: toRef(props, 'submittedMediaTypes'),
   selectedThemes: toRef(props, 'submittedThemes'),
})

/** Organized collections from search results. */
const { visibleCollections } = mediaSearchCollections({
   mediaItems,
   collectionLabel: toRef(props, 'collectionLabel'),
})

/**
 * Forwards the selected media item so the parent can swap to the corresponding detail screen.
 *
 * @param item Media item selected from the result grid.
 */
function handleSelectItem(item: MediaItem) {
   emit('select-item', item)
}

/** Scroll-to-top button state and handler. */
const { showScrollToTop, scrollToTop } = useScrollToTop()
/** Internationalization utilities. */
const { t } = useI18n()
</script>
<template>
   <main class="media-search">
     <section class="media-search__shell">

       <article v-if="errorMessage" class="media-search__empty">
         <h2 class="media-search__empty-title">{{ t('search.errorTitle') }}</h2>
         <p class="media-search__empty-copy">
           {{ errorMessage }}
         </p>
       </article>

       <article v-else-if="isSearching" class="media-search__empty">
         <h2 class="media-search__empty-title">{{ t('entry.loadingTitle') }}</h2>
         <p class="media-search__empty-copy">
           {{ t('entry.loadingMessage') }}
         </p>
       </article>

       <article v-else-if="!hasSearched" class="media-search__empty">
         <h2 class="media-search__empty-title">{{ t('search.label') }}</h2>
         <p class="media-search__empty-copy">
           {{ t('search.startMessage') }}
         </p>
       </article>

       <!-- Keep the empty state in the main layout so search feedback appears where results would normally render. -->
       <article v-else-if="!mediaItems.length" class="media-search__empty">
         <h2 class="media-search__empty-title">{{ t('search.emptyTitle') }}</h2>
         <p class="media-search__empty-copy">
           {{ t('search.emptyMessage') }}
         </p>
       </article>

       <section v-else class="media-search__collections">
         <MediaCardCollection
           v-for="collection in visibleCollections"
           :key="collection.key"
           :items="collection.items"
           :label="collection.label"
           :mode="props.collectionMode"
           :thumbnail-orientation="props.thumbnailOrientation"
           :thumbnail-image-fit="props.thumbnailImageFit"
           :is-loading-more="isLoadingMore"
           :have-more="haveMore"
           :load-more-error-message="loadMoreErrorMessage"
           :on-load-more="loadMore"
           :auto-load-more="true"
           @select="handleSelectItem"
         />
       </section>
     </section>

     <ScrollToTopButton
       v-if="showScrollToTop"
       @click="scrollToTop"
     />
   </main>
 </template>

<style scoped>
.media-search {
   position: relative;
   min-height: 100vh;
   min-height: 100dvh;
   overflow: hidden;
   background: var(--bg-transparent);
   color: var(--text-primary);
 }

.media-search__shell {
   position: relative;
   z-index: 1;
   width: calc(100% - 32px);
   margin: 0 auto;
   padding: 16px 0 32px;
 }

.media-search__empty {
   width: fit-content;
   max-width: min(100%, 560px);
   margin: 0 auto;
   padding: 36px 24px;
   border: 1px solid var(--border-color-primary);
   border-radius: var(--radius);
   background: var(--bg-surface);
   box-shadow: var(--inset-light);
   box-sizing: border-box;
   text-align: center;
   color: var(--text-secondary);
 }

.media-search__empty-title {
   margin-bottom: 8px;
   font-size: 1.4rem;
 }

.media-search__empty-copy {
   color: var(--text-secondary);
 }

.media-search__collections {
   display: grid;
    gap: 28px;
 }

 @media (max-width: 960px) {
   .media-search__shell {
     width: min(100%, calc(100% - 20px));
     padding: 12px 0 24px;
   }
 }

</style>
