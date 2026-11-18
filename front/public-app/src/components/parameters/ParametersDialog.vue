<script setup lang="ts">
import PreferenceEditor from './PreferenceEditor.vue'

/** Controlled settings dialog; each opening starts a fresh server-backed draft. */
defineProps<{ open: boolean }>()
const emit = defineEmits<{ 'update:open': [value: boolean] }>()
</script>

<template>
  <PreferenceEditor v-if="open" @close="emit('update:open', false)" />
</template>
