<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { useServiceMetadata } from '@/composables/useServiceMetadata'
import type { MediaItem, ThumbnailImageFit, ThumbnailOrientation } from '@/types/media'

import MediaCardCardLayout from './media-card/MediaCardCardLayout.vue'
import MediaCardListLayout from './media-card/MediaCardListLayout.vue'

/** Available layout modes for the media card. */
type MediaCardLayout = 'card' | 'list'

/**
 * Props accepted by the media card component.
 */
interface Props {
  /**
   * Media item rendered by the card.
   */
  item: MediaItem
  /**
   * Thumbnail orientation applied to the card layout.
   */
  thumbnailOrientation: ThumbnailOrientation
  /**
   * Poster fit mode applied to the media artwork.
   */
  thumbnailImageFit: ThumbnailImageFit
  /**
   * Layout used to render the media card.
   * @default 'card'
   */
  layout?: MediaCardLayout
  /**
   * Hides the list thumbnail column when the item does not expose any poster.
   * @default false
   */
  hideMissingThumbnail?: boolean
  /**
   * Indicates whether the card preview should currently be visible.
   * @default false
   */
  isPreviewOpen?: boolean
  /**
   * Indicates whether the backend service logo should be displayed on the card poster.
   * @default true
   */
  showServiceLogo?: boolean
}

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
  layout: 'card',
  hideMissingThumbnail: false,
  isPreviewOpen: false,
  showServiceLogo: true,
})

const emit = defineEmits<{
  /** Emitted when the card is selected. */
  select: [item: MediaItem]
  /** Emitted when the preview should be opened. */
  previewOpen: [itemId: string]
  /** Emitted when the preview should be closed. */
  previewClose: [itemId: string]
  /** Emitted when the preview root element changes. */
  previewRootChange: [payload: { itemId: string; element: HTMLElement | null }]
}>()

/** Whether the main image is available for display. */
const imageAvailable = ref(true)
/** Whether the service logo is available for display. */
const serviceLogoAvailable = ref(true)
/** Service metadata utilities. */
const { getService } = useServiceMetadata()

/** Metadata for the service of the current media item. */
const serviceMetadata = computed(() => getService(props.item.source))

/** Display title for the service. */
const serviceTitle = computed(() =>
  serviceMetadata.value?.title?.trim() || props.item.source?.trim() || null,
)

/** URL of the service logo to display. */
const serviceLogo = computed(() =>
  serviceLogoAvailable.value ? serviceMetadata.value?.logo ?? null : null,
)

watch(
  () => props.item.source,
  () => {
    serviceLogoAvailable.value = true
  },
)

/**
 * Indicates whether the card should use the dedicated list layout.
 */
const isListLayout = computed(() => props.layout === 'list')

/**
 * Indicates whether the list layout should keep its poster column visible.
 */
const showListPoster = computed(() =>
  !isListLayout.value || !props.hideMissingThumbnail || Boolean(props.item.imagePosterUrl),
)

/**
 * Provides a stable title even when the media item title is missing or blank.
 */
const displayTitle = computed(() => props.item.title?.trim() || 'Titre indisponible')

/**
 * Indicates whether the card can request loading entry details from the backend.
 */
  const canSelectItem = computed(() => Boolean(props.item.source))

/**
 * Formats the rating for display in the card badge.
 */
const formattedRating = computed(() => props.item.rating?.toFixed(1).replace('.', ',') ?? null)

/**
 * Maps the numeric rating to the corresponding badge variant.
 */
const ratingClass = computed(() => {
  if (props.item.rating && props.item.rating >= 4) {
    return 'media-card__rating--good'
  }

  if (props.item.rating && props.item.rating >= 3) {
    return 'media-card__rating--average'
  }

  return 'media-card__rating--low'
})

/**
 * Switches the card to its visual fallback when the poster image cannot be loaded.
 */
function handleImageError() {
  imageAvailable.value = false
}

/**
 * Hides the service logo for this card when the image cannot be loaded.
 */
function handleServiceLogoError() {
  serviceLogoAvailable.value = false
}

/**
 * Emits the current item when the preview action should load its detailed entry.
 */
function handleSelect() {
  if (!canSelectItem.value) {
    return
  }

  emit('select', props.item)
}

/**
 * Requests opening the preview for the current media card.
 */
function handlePreviewOpen() {
  emit('previewOpen', props.item.id)
}

/**
 * Requests closing the preview for the current media card.
 */
function handlePreviewClose() {
  emit('previewClose', props.item.id)
}

/**
 * Forwards the current card root element to the parent collection preview controller.
 *
 * @param element Current root element rendered by the interactive card layout.
 */
function handlePreviewRootChange(element: HTMLElement | null) {
  emit('previewRootChange', {
    itemId: props.item.id,
    element,
  })
}
</script>

<template>
  <MediaCardListLayout
    v-if="isListLayout"
    :item="item"
    :display-title="displayTitle"
    :can-select-item="canSelectItem"
    :formatted-rating="formattedRating"
    :rating-class="ratingClass"
    :thumbnail-orientation="thumbnailOrientation"
    :thumbnail-image-fit="thumbnailImageFit"
    :show-list-poster="showListPoster"
    :image-available="imageAvailable"
    :service-title="serviceTitle"
    :service-logo="serviceLogo"
    :show-service-logo="showServiceLogo"
    @image-error="handleImageError"
    @service-logo-error="handleServiceLogoError"
    @select="handleSelect"
  />

  <MediaCardCardLayout
    v-else
    :item="item"
    :display-title="displayTitle"
    :can-select-item="canSelectItem"
    :formatted-rating="formattedRating"
    :rating-class="ratingClass"
    :thumbnail-orientation="thumbnailOrientation"
    :thumbnail-image-fit="thumbnailImageFit"
    :image-available="imageAvailable"
    :is-preview-open="isPreviewOpen"
    :service-title="serviceTitle"
    :service-logo="serviceLogo"
    :show-service-logo="showServiceLogo"
    @image-error="handleImageError"
    @service-logo-error="handleServiceLogoError"
    @select="handleSelect"
    @open-preview="handlePreviewOpen"
    @close-preview="handlePreviewClose"
    @root-change="handlePreviewRootChange"
  />
</template>
