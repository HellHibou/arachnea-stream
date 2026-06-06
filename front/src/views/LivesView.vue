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
const parameters = useStorage().getParameters()
const { t } = useI18n()

const emit = defineEmits<{
  'select-live': [item: MediaItem]
}>()

const livePayload = computed(() =>
  props.encodedLive ? decodeLiveRoutePayload(props.encodedLive) : null,
)
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
