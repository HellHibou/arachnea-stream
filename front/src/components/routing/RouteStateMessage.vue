<script setup lang="ts">
/**
 * Props accepted by a compact route-level state message.
 */
interface Props {
  /**
   * Main state title displayed to the user.
   */
  title: string
  /**
   * Supporting state message.
   */
  message: string
}

defineProps<Props>()
</script>

<template>
  <main class="route-state">
    <article class="route-state__panel">
      <h1 class="route-state__title">{{ title }}</h1>
      <p class="route-state__message">{{ message }}</p>
    </article>
  </main>
</template>

<style scoped>
.route-state {
  display: grid;
  place-items: center;
  min-height: calc(100vh - 124px);
  min-height: calc(100dvh - 124px);
  padding: 16px 0 32px;
  color: var(--text-primary);
}

.route-state__panel {
  width: fit-content;
  max-width: min(100%, 620px);
  padding: 36px 24px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  box-shadow: var(--inset-light);
  color: var(--text-secondary);
  text-align: center;
}

.route-state__title {
  margin-bottom: 8px;
  color: var(--text-primary);
  font-size: 1.4rem;
}

.route-state__message {
  color: var(--text-secondary);
}

@media (max-width: 640px) {
  .route-state {
    min-height: calc(100vh - 110px);
    min-height: calc(100dvh - 110px);
  }
}
</style>
