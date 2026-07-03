<script setup lang="ts">
import { computed, shallowRef, watch } from 'vue'
import { useRoute } from 'vue-router'

import MediaSearch from '@/components/MediaSearch.vue'
import { readSearchRouteQuery } from '@/router/searchQuery'
import { useStorage } from '@/services/storage'
import type { MediaCardCollectionMode, MediaItem } from '@/types/media'

/** Vue Router current route. */
const route = useRoute()

/** Application parameters loaded from persistent storage. */
const parameters = useStorage().getParameters()

/** Counter to trigger new search requests when route query changes. */
const searchRequestId = shallowRef(0)

/** Collection mode for search results, excluding single-row mode. */
const searchCollectionMode = computed<Exclude<MediaCardCollectionMode, 'single-row'>>(() =>
  parameters.collectionMode.value === 'list' ? 'list' : 'grid',
)

const emit = defineEmits<{
  /** Emitted when a media item is selected from search results. */
  'select-item': [item: MediaItem]
}>()

/** Search state extracted from the current route query parameters. */
const routeSearchState = computed(() => readSearchRouteQuery(route.query))

watch(
  () => JSON.stringify(routeSearchState.value),
  () => {
    searchRequestId.value += 1
  },
  { immediate: true },
)
</script>

<template>
  <MediaSearch
    :submitted-query="routeSearchState.query"
    :search-request-id="searchRequestId"
    :thumbnail-orientation="parameters.thumbnailOrientation.value"
    :thumbnail-image-fit="parameters.thumbnailImageFit.value"
    :collection-mode="searchCollectionMode"
    :submitted-media-types="routeSearchState.mediaTypes"
    :submitted-themes="routeSearchState.themes"
    collection-label="Resultat de recherche"
    @select-item="emit('select-item', $event)"
  />
</template>
