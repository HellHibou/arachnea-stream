<script setup lang="ts">
import { computed, shallowRef, watch } from 'vue'
import { useRoute } from 'vue-router'

import MediaSearch from '@/components/MediaSearch.vue'
import { readSearchRouteQuery } from '@/router/searchQuery'
import { useStorage } from '@/services/storage'
import type { MediaCardCollectionMode, MediaItem } from '@/types/media'

const route = useRoute()
const parameters = useStorage().getParameters()
const searchRequestId = shallowRef(0)
const searchCollectionMode = computed<Exclude<MediaCardCollectionMode, 'single-row'>>(() =>
  parameters.collectionMode.value === 'list' ? 'list' : 'grid',
)

const emit = defineEmits<{
  'select-item': [item: MediaItem]
}>()

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
