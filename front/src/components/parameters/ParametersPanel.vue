<script setup lang="ts">
/**
 * Props accepted by one reusable parameters row.
 */
interface Props {
  /**
   * Title rendered on the left side of the row.
   */
  title: string
  /**
   * Indicates whether the row is disabled.
   * @default false
   */
  disabled?: boolean
}

withDefaults(defineProps<Props>(), {
  disabled: false,
})
</script>

<template>
  <div
    class="parameters__panel"
    :class="{ 'parameters__panel--disabled': disabled }"
    :aria-disabled="disabled || undefined"
  >
    <div class="parameters__copy">
      <p class="parameters__panel-title">
        {{ title }}
      </p>
    </div>

    <slot />
  </div>
</template>

<style scoped>
.parameters__panel {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 1px 0;
}

.parameters__panel--disabled {
  opacity: 0.42;
}

.parameters__copy {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 0;
}

.parameters__panel-title {
  margin-left: 10px;
  color: var(--text-primary);
  font-size: 0.92rem;
  font-weight: 700;
  line-height: 1.2;
}

@media (max-width: 720px) {
  .parameters__panel {
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
  }
}
</style>
