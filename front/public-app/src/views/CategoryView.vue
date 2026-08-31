<script setup lang="ts">
import { computed } from 'vue'

import HomeCatalog from '@/components/HomeCatalog.vue'
import RouteStateMessage from '@/components/routing/RouteStateMessage.vue'
import { decodeCategoryRoutePayload } from '@/router/routePayloads'
import { useStorage } from '@/services/storage'
import { useI18n } from '@/i18n'
import type { HomeCategory } from '@/types/home'
import type { BackgroundMediaCandidate, MediaSelectionTarget } from '@/types/media'

/**
 * Props accepted by the category route view.
 */
interface Props {
  /**
   * Informative category slug and base64url payload token from the route path.
   */
  categoryToken: string
}

const props = defineProps<Props>()

/** Application parameters loaded from persistent storage. */
const parameters = useStorage().getParameters()
const { t } = useI18n()

const emit = defineEmits<{
  /** Emitted when a media item is selected. */
  'select-item': [target: MediaSelectionTarget]
  /** Emitted when a category is selected. */
  'select-category': [category: HomeCategory]
  /** Emitted when background media items are updated. */
  'update:background-media-items': [mediaItems: BackgroundMediaCandidate[]]
}>()

/**
 * Category rebuilt synchronously from the base64url route payload.
 *
 * Null when the route parameter cannot be decoded, which renders the
 * invalid-link error state instead of the catalog.
 */
const category = computed<HomeCategory | null>(() => {
  const payload = decodeCategoryRoutePayload(props.categoryToken)

  if (!payload) {
    return null
  }

  const separatorIndex = props.categoryToken.indexOf('-')
  const slug = props.categoryToken.slice(0, separatorIndex)

  return {
    id: slug,
    label: payload.label,
    imageUrl: null,
    mergeKey: slug,
    sources: payload.sources,
  }
})
</script>

<template>
  <RouteStateMessage
    v-if="!category"
    :title="t('category.unavailableTitle')"
    :message="t('category.invalidLink')"
  />

  <HomeCatalog
    v-else
    mode="category"
    :category="category"
    :thumbnail-orientation="parameters.thumbnailOrientation.value"
    :thumbnail-image-fit="parameters.thumbnailImageFit.value"
    :thumbnail-size-multiplier="parameters.thumbnailSizeMultiplier.value"
    @select-item="emit('select-item', $event)"
    @select-category="emit('select-category', $event)"
    @update:background-media-items="emit('update:background-media-items', $event)"
  />
</template>
