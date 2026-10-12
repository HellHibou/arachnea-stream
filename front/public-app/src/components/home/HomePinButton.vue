<script setup lang="ts">
/** Accessible compact pin action; label is required, active defaults to false. */
withDefaults(defineProps<{ label: string; active?: boolean }>(), { active: false })
const emit = defineEmits<{ click: [] }>()
</script>

<template>
  <button class="home-pin-button" type="button" :aria-label="label" :title="label"
    @click.stop.prevent="emit('click')">
    <v-icon :icon="active ? 'mdi-pin' : 'mdi-pin-outline'" size="18" aria-hidden="true" />
  </button>
</template>

<style scoped>
.home-pin-button {
  display: inline-grid;
  place-items: center;
  width: 36px;
  min-width: 36px;
  height: 36px;
  padding: 0;
  border: 1px solid var(--border-color-primary);
  border-radius: 999px;
  background: rgb(20 20 20 / 55%);
  color: var(--text-secondary);
  cursor: pointer;
  box-shadow: var(--inset-light);
}

.home-pin-button:hover {
  border-color: var(--color-primary);
  color: var(--text-primary);
}

.home-pin-button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>