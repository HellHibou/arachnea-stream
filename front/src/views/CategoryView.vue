<script setup lang="ts">
import { shallowRef, watch } from 'vue'

import HomeCatalog from '@/components/HomeCatalog.vue'
import RouteStateMessage from '@/components/routing/RouteStateMessage.vue'
import { loadHomeCatalog } from '@/services/rustify'
import { useStorage } from '@/services/storage'
import { useI18n } from '@/i18n'
import type { HomeCategory } from '@/types/home'
import type { BackgroundMediaCandidate, MediaSelectionTarget } from '@/types/media'

/**
 * Props accepted by the category route view.
 */
interface Props {
  /**
   * Public category merge key from the route path.
   */
  categoryKey: string
}

const props = defineProps<Props>()

/** Application parameters loaded from persistent storage. */
const parameters = useStorage().getParameters()
const { t } = useI18n()

/** Resolved category from the route parameter. */
const category = shallowRef<HomeCategory | null>(null)

/** Whether a category resolution is currently in progress. */
const isResolvingCategory = shallowRef(false)

/** Error message displayed when category resolution fails. */
const errorMessage = shallowRef<string | null>(null)

/** Counter to track the latest category resolution request. */
let latestRequestId = 0

const emit = defineEmits<{
  /** Emitted when a media item is selected. */
  'select-item': [target: MediaSelectionTarget]
  /** Emitted when a category is selected. */
  'select-category': [category: HomeCategory]
  /** Emitted when background media items are updated. */
  'update:background-media-items': [mediaItems: BackgroundMediaCandidate[]]
}>()

/**
 * Resolves the public category key into a full HomeCategory.
 *
 * @param categoryKey Category merge key received from the route path.
 */
async function resolveCategory(categoryKey: string) {
  const requestId = ++latestRequestId
  const normalizedCategoryKey = categoryKey.trim()

  category.value = null
  errorMessage.value = null
  emit('update:background-media-items', [])

  if (!normalizedCategoryKey) {
    errorMessage.value = t('category.invalidLink')
    return
  }

  isResolvingCategory.value = true

  try {
    const homeCatalog = await loadHomeCatalog()

    if (requestId !== latestRequestId) {
      return
    }

    const matchingCategory = homeCatalog.categories.find(
      (candidate) => candidate.mergeKey === normalizedCategoryKey,
    )

    if (!matchingCategory) {
      errorMessage.value = t('category.notFound')
      return
    }

    category.value = matchingCategory
  } catch (error) {
    if (requestId !== latestRequestId) {
      return
    }

    errorMessage.value =
      error instanceof Error ? error.message : t('errors.homeCatalog')
  } finally {
    if (requestId === latestRequestId) {
      isResolvingCategory.value = false
    }
  }
}

watch(
  () => props.categoryKey,
  (categoryKey) => {
    void resolveCategory(categoryKey)
  },
  { immediate: true },
)
</script>

<template>
  <RouteStateMessage
    v-if="errorMessage"
    :title="t('category.unavailableTitle')"
    :message="errorMessage"
  />

  <RouteStateMessage
    v-else-if="isResolvingCategory || !category"
    :title="t('category.loadingTitle')"
    :message="t('category.loadingMessage')"
  />

  <HomeCatalog
    v-else
    mode="category"
    :category="category"
    :thumbnail-orientation="parameters.thumbnailOrientation.value"
    :thumbnail-image-fit="parameters.thumbnailImageFit.value"
    @select-item="emit('select-item', $event)"
    @select-category="emit('select-category', $event)"
    @update:background-media-items="emit('update:background-media-items', $event)"
  />
</template>
