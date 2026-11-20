<script setup lang="ts">
import { useI18n } from '@/i18n'

defineProps<{
  /** Block label used to identify its pagination and status controls. */
  title: string
  /** Show the label when the block has no visible section heading. */
  showTitle: boolean
  /** Whether the category has exhausted its pages without media. */
  empty: boolean
  /** Whether a category page is pending. */
  loading: boolean
  /** Whether the category can request another page. */
  haveMore: boolean
  /** The error for this category request, when present. */
  error?: string
  /** Whether the saved reference needs editing. */
  repairRequired?: boolean
  /** Whether the user can add a shortcut from this block. */
  canAddShortcut: boolean
}>()

const emit = defineEmits<{ loadPage: []; addShortcut: [] }>()
const { t } = useI18n()
</script>

<template>
  <div v-if="empty || loading || haveMore || error" class="custom-home-category-status" role="group" :aria-label="title">
    <h2 v-if="showTitle" class="custom-home-category-status__title">{{ title }}</h2>
    <p v-if="loading" role="status">{{ t('entry.loadingMessage') }}</p>
    <p v-else-if="error" role="alert">
      {{ error }}
      <span v-if="repairRequired">{{ t('customHome.repairRequired') }}</span>
    </p>
    <p v-else-if="empty" role="status">{{ t('customHome.emptyCategoryContent') }}</p>
    <v-btn v-if="error || haveMore" :loading="loading" :disabled="loading" variant="tonal" @click="emit('loadPage')">
      {{ t(error ? 'sourceHome.retry' : 'customHome.loadMoreCategory') }}
    </v-btn>
    <v-btn v-if="empty && canAddShortcut" variant="tonal" @click="emit('addShortcut')">
      {{ t('customHome.addShortcutTooltip') }}
    </v-btn>
  </div>
</template>

<style scoped>
.custom-home-category-status {
  display: grid;
  justify-items: start;
  gap: 8px;
}

.custom-home-category-status__title {
  font-size: 1.25rem;
}
</style>
