<script setup lang="ts">
import Parameters from '@/components/Parameters.vue'
import { useI18n } from '@/i18n'
/** Controlled settings dialog; existing controls retain immediate preference persistence. */
defineProps<{ open: boolean }>()
const emit = defineEmits<{ 'update:open': [value: boolean] }>()
const { t } = useI18n()
</script>

<template>
  <v-dialog :model-value="open" max-width="720" scrollable class="parameters-dialog"
    @update:model-value="emit('update:open', $event)">
    <v-card class="parameters-dialog__surface" :title="t('navigation.settings')">
      <v-card-text><Parameters /></v-card-text>
      <v-card-actions><button class="parameters-dialog__close" type="button"
        @click="emit('update:open', false)">{{ t('customHome.close') }}</button></v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.parameters-dialog__surface {
  background: var(--bg-surface);
  color: var(--text-primary);
  font-family: inherit;
}

.parameters-dialog__surface :deep(.v-card-title) {
  font-weight: 700;
}

.parameters-dialog__close {
  padding: 10px 18px;
  border: 0;
  border-radius: 999px;
  background: var(--color-primary);
  color: var(--text-primary);
  font: inherit;
  cursor: pointer;
}

.parameters-dialog__close:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.parameters-dialog :deep(.v-overlay__scrim) {
  backdrop-filter: var(--backdrop-filter-strong);
  opacity: 1;
  background: rgb(0 0 0 / 45%);
}
</style>