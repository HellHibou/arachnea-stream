<script setup lang="ts">
import HomeCatalog from '@/components/HomeCatalog.vue'
import { useStorage } from '@/services/storage'
import type { HomeCategory } from '@/types/home'
import type {
  BackgroundMediaCandidate,
  MediaSelectionTarget,
} from '@/types/media'

/** Application parameters loaded from persistent storage. */
const parameters = useStorage().getParameters()

const emit = defineEmits<{
  /** Emitted when a media item is selected. */
  'select-item': [target: MediaSelectionTarget]
  /** Emitted when a category is selected. */
  'select-category': [category: HomeCategory]
  /** Emitted when background media items are updated. */
  'update:background-media-items': [mediaItems: BackgroundMediaCandidate[]]
}>()
</script>

<template>
  <HomeCatalog
    mode="home"
    :thumbnail-orientation="parameters.thumbnailOrientation.value"
    :thumbnail-image-fit="parameters.thumbnailImageFit.value"
    :thumbnail-size-multiplier="parameters.thumbnailSizeMultiplier.value"
    @select-item="emit('select-item', $event)"
    @select-category="emit('select-category', $event)"
    @update:background-media-items="emit('update:background-media-items', $event)"
  />
</template>
