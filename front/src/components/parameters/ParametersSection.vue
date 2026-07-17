<script setup lang="ts">
/**
 * Props accepted by one reusable parameters section shell.
 */
interface Props {
  /**
   * Title rendered in the section header.
   */
  title: string
}

defineProps<Props>()
</script>

<template>
  <section class="parameters__group">
    <header class="parameters__group-header">
      <h3 class="parameters__group-title">
        {{ title }}
      </h3>
    </header>

    <slot />
  </section>
</template>

<style scoped>
.parameters__group {
  --parameters-control-width: 272px;
  display: grid;
  gap: 8px;
  padding: 14px 16px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  box-shadow: var(--inset-light);
}

.parameters__group-header {
  display: flex;
  align-items: center;
  min-height: 30px;
  padding: 0 2px 10px;
  border-bottom: 1px solid var(--border-color-primary);
}

.parameters__group-title {
  position: relative;
  padding-left: 12px;
  color: var(--text-primary);
  font-size: 1.16rem;
  font-weight: 800;
  letter-spacing: 0.01em;
  line-height: 1.2;
}

.parameters__group-title::before {
  content: '';
  position: absolute;
  top: 50%;
  left: 0;
  width: 4px;
  height: 18px;
  border-radius: 999px;
  transform: translateY(-50%);
  background: var(--bg-accent-marker-primary);
}
</style>
