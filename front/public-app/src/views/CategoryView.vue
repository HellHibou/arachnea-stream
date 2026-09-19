<script setup lang="ts">
import { computed, onUnmounted, watch } from 'vue'

import HomeCatalog from '@/components/HomeCatalog.vue'
import RouteStateMessage from '@/components/routing/RouteStateMessage.vue'
import { decodeCategoryRoutePayload } from '@/router/routePayloads'
import { useStorage } from '@/services/storage'
import { useI18n } from '@/i18n'
import { APP_TITLE } from '@/constants'
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

/** Builds the browser tab title from the category label and the application title. */
const pageTitle = computed(() => {
  const label = category.value?.label
  return label ? `${label} - ${APP_TITLE}` : APP_TITLE
})

/** Updates the browser tab title whenever the page title changes. */
watch(pageTitle, (title) => {
  document.title = title
}, { immediate: true })

/** Restores the default application title when the component is unmounted. */
onUnmounted(() => {
  document.title = APP_TITLE
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
