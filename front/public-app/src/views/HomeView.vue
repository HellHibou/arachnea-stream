<script setup lang="ts">
import { computed, onUnmounted, watch } from 'vue'
import HomeCatalog from '@/components/HomeCatalog.vue'
import { useServiceMetadata } from '@/composables/useServiceMetadata'
import { APP_TITLE } from '@/constants'
import { useStorage } from '@/services/storage'
import type { HomeCategory } from '@/types/home'
import type {
  BackgroundMediaCandidate,
  MediaSelectionTarget,
} from '@/types/media'

/** Application parameters loaded from persistent storage. */
const parameters = useStorage().getParameters()

/** Source route input; null preserves the aggregated home. @default null */
const props = withDefaults(defineProps<{ sourceId?: string | null }>(), { sourceId: null })
const metadata = useServiceMetadata()
const pageTitle = computed(() => props.sourceId === null ? APP_TITLE : `${metadata.getService(props.sourceId)?.title ?? props.sourceId} - ${APP_TITLE}`)
watch(pageTitle, (title) => { document.title = title }, { immediate: true })
onUnmounted(() => { document.title = APP_TITLE })

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
    :key="JSON.stringify(props.sourceId)"
    mode="home"
    :source-id="props.sourceId"
    :thumbnail-orientation="parameters.thumbnailOrientation.value"
    :thumbnail-image-fit="parameters.thumbnailImageFit.value"
    :thumbnail-size-multiplier="parameters.thumbnailSizeMultiplier.value"
    @select-item="emit('select-item', $event)"
    @select-category="emit('select-category', $event)"
    @update:background-media-items="emit('update:background-media-items', $event)"
  />
</template>
