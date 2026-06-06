<script setup lang="ts">
import { computed } from 'vue'

import { useI18n } from '@/i18n'

/**
 * Props accepted by the floating scroll-to-top control.
 */
interface Props {
  /**
   * Accessible label and tooltip text.
   * @default 'Remonter en haut'
   */
  title?: string
}

const props = withDefaults(defineProps<Props>(), {
  title: undefined,
})
const { t } = useI18n()
const buttonTitle = computed(() => props.title ?? t('toolbar.scrollTop'))

const emit = defineEmits<{
  click: []
}>()
</script>

<template>
  <button
    class="scroll-to-top-button"
    type="button"
    :aria-label="buttonTitle"
    :title="buttonTitle"
    @click="emit('click')"
  >
    <v-icon icon="mdi-chevron-up" size="24" aria-hidden="true" />
  </button>
</template>

<style scoped>
.scroll-to-top-button {
  position: fixed;
  right: 14px;
  bottom: 14px;
  z-index: 1000;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 44px;
  min-width: 44px;
  height: 44px;
  padding: 0;
  border: 1px solid var(--border-color-primary);
  border-radius: 999px;
  background: var(--bg-surface);
  color: var(--text-primary);
  font-size: 1rem;
  font-weight: 800;
  cursor: pointer;
  opacity: 0.5;
  transition:
    transform var(--duration-fast) ease,
    filter var(--duration-fast) ease,
    opacity var(--duration-fast) ease;
}

.scroll-to-top-button:hover:not(:disabled) {
  transform: translateY(-1px);
  filter: brightness(1.06);
}

.scroll-to-top-button:hover {
  transform: scale(1.1);
  opacity: 1;
}

.scroll-to-top-button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>
