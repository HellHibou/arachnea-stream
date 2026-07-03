<script setup lang="ts">
import { computed } from 'vue'

import ProgramEntryDetails from '@/components/ProgramEntryDetails.vue'
import RouteStateMessage from '@/components/routing/RouteStateMessage.vue'
import { decodeEntryRoutePayload } from '@/router/routePayloads'
import { useStorage } from '@/services/storage'
import { useI18n } from '@/i18n'

/**
 * Props accepted by the entry detail route view.
 */
interface Props {
  /**
   * Base64url entry payload from the route path.
   */
  encodedEntry: string
}

const props = defineProps<Props>()

/** Application parameters loaded from persistent storage. */
const parameters = useStorage().getParameters()
const { t } = useI18n()

/** Decoded entry payload from the route path. */
const entryPayload = computed(() => decodeEntryRoutePayload(props.encodedEntry))
</script>

<template>
  <RouteStateMessage
    v-if="!entryPayload"
    :title="t('entry.unavailableTitle')"
    :message="t('entry.unavailableMessage')"
  />

  <ProgramEntryDetails
    v-else
    :source="entryPayload.source"
    :entry="entryPayload.entryUrl"
    :web-url="null"
    :use-trailer-as-background="parameters.useTrailerAsBackground.value"
    :is-background-animated="parameters.isBackgroundAnimated.value"
    :background-image-fit="parameters.backgroundImageFit.value"
    :use-catalog-banners-as-background="parameters.useCatalogBannersAsBackground.value"
  />
</template>
