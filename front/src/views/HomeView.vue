<script setup lang="ts">
import HomeCatalog from '@/components/HomeCatalog.vue'
import { useStorage } from '@/services/storage'
import type { HomeCategory } from '@/types/home'
import type {
  BackgroundMediaCandidate,
  MediaSelectionTarget,
} from '@/types/media'

const parameters = useStorage().getParameters()

const emit = defineEmits<{
  'select-item': [target: MediaSelectionTarget]
  'select-category': [category: HomeCategory]
  'update:background-media-items': [mediaItems: BackgroundMediaCandidate[]]
}>()
</script>

<template>
  <HomeCatalog
    mode="home"
    :thumbnail-orientation="parameters.thumbnailOrientation.value"
    :thumbnail-image-fit="parameters.thumbnailImageFit.value"
    @select-item="emit('select-item', $event)"
    @select-category="emit('select-category', $event)"
    @update:background-media-items="emit('update:background-media-items', $event)"
  />
</template>
