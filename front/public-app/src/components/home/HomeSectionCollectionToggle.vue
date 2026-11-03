<script setup lang="ts">
import { computed } from 'vue'

import type { MediaCardCollectionMode } from '@/types/media'
import { useI18n } from '@/i18n'

/**
 * Props accepted by the section collection layout toggle.
 */
interface Props {
  /**
   * Current collection mode of the section.
   */
  mode: MediaCardCollectionMode
}

const props = defineProps<Props>()

const emit = defineEmits<{
  /** Emitted when the section should switch between single-row and multi-row layouts. */
  toggle: []
}>()

/** Internationalization utilities. */
const { t } = useI18n()
/** Whether the current mode supports the single-row/multi-row toggle. */
const canToggle = computed(() => props.mode === 'single-row' || props.mode === 'grid')
/** Whether the section is currently collapsed into a single row. */
const isSingleRow = computed(() => props.mode === 'single-row')
/** Resolved accessible label and tooltip for the toggle. */
const label = computed(() =>
  isSingleRow.value ? t('catalog.expandToMultipleRows') : t('catalog.collapseToSingleRow'),
)
</script>

<template>
  <button
    v-if="canToggle"
    class="home-section-collection-toggle"
    type="button"
    :aria-label="label"
    :aria-expanded="!isSingleRow"
    :title="label"
    @click="emit('toggle')"
  >
    <v-icon
      :icon="isSingleRow ? 'mdi-chevron-down' : 'mdi-chevron-up'"
      size="20"
      aria-hidden="true"
    />
  </button>
</template>

<style scoped>
.home-section-collection-toggle {
  display: grid;
  width: 32px;
  height: 32px;
  place-items: center;
  border: 1px solid var(--border-color-primary);
  border-radius: 50%;
  background: var(--bg-surface);
  color: var(--text-primary);
  cursor: pointer;
}

.home-section-collection-toggle:hover,
.home-section-collection-toggle:focus-visible {
  border-color: var(--color-primary);
  background: var(--bg-surface-strong);
}

.home-section-collection-toggle:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>
