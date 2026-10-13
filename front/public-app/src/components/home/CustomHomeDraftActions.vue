<script setup lang="ts">
import { useI18n } from '@/i18n'
/** Position and list length determine movement limits; actions only modify the owning draft. */
withDefaults(defineProps<{ index: number; count: number; hideRemove?: boolean }>(), { hideRemove: false })
const emit = defineEmits<{ move: [direction: -1 | 1]; remove: [] }>()
const { t } = useI18n()
</script>

<template>
  <span class="draft-actions">
    <button class="draft-actions__button" type="button" :disabled="index === 0"
      :aria-label="t('customHome.moveUp')" :title="t('customHome.moveUp')" @click="emit('move', -1)">
      <v-icon icon="mdi-arrow-up" size="18" aria-hidden="true" />
    </button>
    <button class="draft-actions__button" type="button" :disabled="index === count - 1"
      :aria-label="t('customHome.moveDown')" :title="t('customHome.moveDown')" @click="emit('move', 1)">
      <v-icon icon="mdi-arrow-down" size="18" aria-hidden="true" />
    </button>
    <button v-if="!hideRemove" class="draft-actions__button" type="button" :aria-label="t('customHome.remove')"
      :title="t('customHome.remove')" @click="emit('remove')">
      <v-icon icon="mdi-trash-can-outline" size="18" aria-hidden="true" />
    </button>
  </span>
</template>

<style scoped>
.draft-actions {
  display: inline-flex;
  gap: 6px;
  align-items: center;
}

.draft-actions__button {
  display: inline-grid;
  place-items: center;
  width: 36px;
  height: 36px;
  border: 1px solid var(--border-color-primary);
  border-radius: 999px;
  background: var(--bg-transparent);
  color: var(--text-secondary);
  box-shadow: var(--inset-light);
  cursor: pointer;
}

.draft-actions__button:hover:not(:disabled) {
  border-color: var(--color-primary);
  color: var(--text-primary);
}

.draft-actions__button:disabled {
  opacity: 0.42;
  cursor: not-allowed;
}

.draft-actions__button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>