<script setup lang="ts">
import { computed, onUnmounted, toRef, watch } from 'vue'
import HomeCatalog from '@/components/HomeCatalog.vue'
import { useCustomHomeDefinition } from '@/composables/home/useCustomHomeDefinition'
import { useStorage } from '@/services/storage'
import { APP_TITLE } from '@/constants'
import { useI18n } from '@/i18n'
import { reportFrontendErrorOnce } from '@/composables/useErrorNotifications'
import type { HomeCategory } from '@/types/home'
import type { BackgroundMediaCandidate, MediaSelectionTarget } from '@/types/media'
import type { CustomHomeDefinition } from '@/types/customHome'

/** The home ID, optional definition already read by the route guard, and route error (null by default). */
const props = withDefaults(defineProps<{ homeId: string; resolvedHome?: CustomHomeDefinition; routeError?: string | null }>(), { routeError: null })
const emit = defineEmits<{
  'select-item': [target: MediaSelectionTarget]
  'select-category': [category: HomeCategory]
  'update:background-media-items': [items: BackgroundMediaCandidate[]]
}>()
const { definition, errorMessage, isLoading } = useCustomHomeDefinition(toRef(props, 'homeId'), () => props.resolvedHome)
const parameters = useStorage().getParameters()
const { t } = useI18n()
/** Reports custom-home resolution failures through the shared bottom-right notification stack. */
watch(computed(() => props.routeError ?? errorMessage.value), (message) => {
  if (message) reportFrontendErrorOnce(t('errors.notifications.operations.customHomeDefinition'), message)
}, { immediate: true })
const pageTitle = computed(() => definition.value ? `${definition.value.name} - ${APP_TITLE}` : APP_TITLE)
watch(pageTitle, (title) => { document.title = title }, { immediate: true })
watch(definition, (value) => { if (!value) emit('update:background-media-items', []) })
onUnmounted(() => { document.title = APP_TITLE })
</script>

<template>
  <!-- Definition failures are reported through the shared notification stack. -->
  <p v-if="isLoading && !definition" role="status">{{ t('entry.loadingMessage') }}</p>
  <template v-else-if="definition">
    <HomeCatalog
      :key="definition.id"
      mode="custom"
      :custom-definition="definition"
      :thumbnail-orientation="parameters.thumbnailOrientation.value"
      :thumbnail-image-fit="parameters.thumbnailImageFit.value"
      :thumbnail-size-multiplier="parameters.thumbnailSizeMultiplier.value"
      @select-item="emit('select-item', $event)"
      @select-category="emit('select-category', $event)"
      @update:background-media-items="emit('update:background-media-items', $event)"
    />
  </template>
</template>
