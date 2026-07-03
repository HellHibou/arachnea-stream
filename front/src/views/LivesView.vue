<script setup lang="ts">
import { computed } from 'vue'

import LiveEntryDetails from '@/components/LiveEntryDetails.vue'
import RouteStateMessage from '@/components/routing/RouteStateMessage.vue'
import { decodeLiveRoutePayload } from '@/router/routePayloads'
import { useStorage } from '@/services/storage'
import { useI18n } from '@/i18n'
import type { MediaItem } from '@/types/media'

/**
 * Props accepted by the live route view.
 */
interface Props {
  /**
   * Optional base64url live payload from the route path.
   * @default ''
   */
  encodedLive?: string
}

const props = withDefaults(defineProps<Props>(), {
  encodedLive: '',
})

/** Application parameters loaded from persistent storage. */
const parameters = useStorage().getParameters()
const { t } = useI18n()

const emit = defineEmits<{
  /** Emitted when a live stream is selected. */
  'select-live': [item: MediaItem]
}>()

/** Decoded live payload from the route path. */
const livePayload = computed(() =>
  props.encodedLive ? decodeLiveRoutePayload(props.encodedLive) : null,
)

/** Whether the live payload from the route is invalid. */
const isInvalidLivePayload = computed(() => Boolean(props.encodedLive) && !livePayload.value)
</script>

<template>
  <RouteStateMessage
    v-if="isInvalidLivePayload"
    :title="t('live.unavailableTitle')"
    :message="t('live.unavailableMessage')"
  />

  <LiveEntryDetails
    v-else
    :initial-source="livePayload?.source ?? null"
    :initial-channel="livePayload?.channel ?? null"
    :is-background-animated="parameters.isBackgroundAnimated.value"
    :background-image-fit="parameters.backgroundImageFit.value"
    :use-catalog-banners-as-background="parameters.useCatalogBannersAsBackground.value"
    @select-live="emit('select-live', $event)"
  />
</template>
