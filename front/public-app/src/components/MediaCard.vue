<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'

import { useServiceMetadata } from '@/composables/useServiceMetadata'
import type { CatalogItem, MediaItem, ThumbnailImageFit, ThumbnailOrientation } from '@/types/media'
import { mediaPresentation, mediaRoute } from '@/services/mediaRouting'

import MediaCardCardLayout from './media-card/MediaCardCardLayout.vue'
import MediaCardListLayout from './media-card/MediaCardListLayout.vue'

/** Available layout modes for the media card. */
type MediaCardLayout = 'card' | 'list'

/**
 * Props accepted by the media card component.
 */
interface Props {
  /**
   * Catalog or locally selectable item rendered by the component.
   */
  item: CatalogItem
  /**
   * Selects a route target or an item handled locally by the parent.
   * Local items are available when their mapper supplies a nonempty source.
   * @default 'route'
   */
  selectionMode?: 'route' | 'local'
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
  selectionMode: 'route',
  layout: 'card',
  hideMissingThumbnail: false,
  isPreviewOpen: false,
  showServiceLogo: true,
})

const emit = defineEmits<{
  /** Emitted when an available route target or local item is selected. */
  select: [item: MediaItem]
  /** Emitted when the preview should be opened. */
  previewOpen: [itemId: string]
  /** Emitted when the preview should be closed. */
  previewClose: [itemId: string]
  /** Emitted when the preview root element changes. */
  previewRootChange: [payload: { itemId: string; element: HTMLElement | null }]
  /** Emitted when the popup root element changes. */
  previewPopupRootChange: [payload: { itemId: string; element: HTMLElement | null }]
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
  !isListLayout.value || !props.hideMissingThumbnail || Boolean(props.item.imagePosterUrl ?? props.item.imageLandscapeUrl),
)

/**
 * Provides a stable title even when the media item title is missing or blank.
 */
const displayTitle = computed(() => props.item.title?.trim() || 'Titre indisponible')

/** Native destination for catalog navigation; local selections use buttons. */
const destination = computed(() => props.selectionMode === 'route' ? mediaRoute(props.item) : null)
/** Local item mappers retain the source only when the item can be selected. */
const canSelectItem = computed(() => props.selectionMode === 'local'
  ? Boolean(props.item.source?.trim())
  : Boolean(destination.value),
)
/** Shared presentation associated with the registered media type. */
const presentation = computed(() => mediaPresentation(props.item))
/** Vue Router instance used to resolve the native card link. */
const router = useRouter()
/** Native destination for ordinary, modified and context-menu link navigation. */
const actionHref = computed(() => destination.value ? router.resolve(destination.value).href : null)

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
 * Emits a selectable card for route navigation or local handling.
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

/**
 * Forwards the popup root element to the parent collection preview controller.
 *
 * @param element Current popup root element rendered by the preview panel.
 */
function handlePopupRootChange(element: HTMLElement | null) {
  emit('previewPopupRootChange', {
    itemId: props.item.id,
    element,
  })
}
</script>

<template>
  <MediaCardListLayout
    v-if="isListLayout"
    :item="item"
    :stacked-artwork="presentation.stackedArtwork"
    :display-title="displayTitle"
    :can-select-item="canSelectItem"
    :formatted-rating="formattedRating"
    :rating-class="ratingClass"
    :thumbnail-orientation="thumbnailOrientation"
    :thumbnail-image-fit="thumbnailImageFit"
    :show-list-poster="showListPoster"
    :action-label="presentation.actionLabel"
    :image-available="imageAvailable"
    :service-title="serviceTitle"
    :service-logo="serviceLogo"
    :show-service-logo="showServiceLogo"
    :href="actionHref"
    @image-error="handleImageError"
    @service-logo-error="handleServiceLogoError"
    @select="handleSelect"
  />

  <MediaCardCardLayout
    v-else
    :item="item"
    :stacked-artwork="presentation.stackedArtwork"
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
    :href="actionHref"
    @image-error="handleImageError"
    @service-logo-error="handleServiceLogoError"
    @select="handleSelect"
    @open-preview="handlePreviewOpen"
    @close-preview="handlePreviewClose"
    @root-change="handlePreviewRootChange"
    @popup-root-change="handlePopupRootChange"
  />
</template>
