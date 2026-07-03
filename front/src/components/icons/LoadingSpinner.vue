<script setup lang="ts" generic="size = 56">
defineProps<{
  size?: number
}>()
</script>

<template>
  <svg
    class="entry-details__player-loading-spinner"
    viewBox="0 0 50 50"
    xmlns="http://www.w3.org/2000/svg"
    :width="size"
    :height="size"
    aria-hidden="true"
  >
    <circle
      class="entry-details__player-loading-spinner-track"
      cx="25"
      cy="25"
      r="20"
      fill="none"
      stroke-width="4"
    />
    <circle
      class="entry-details__player-loading-spinner-head"
      cx="25"
      cy="25"
      r="20"
      fill="none"
      stroke-width="4"
      stroke-linecap="round"
    />
  </svg>
</template>

<style scoped>
.entry-details__player-loading-spinner {
  animation: entry-details-spinner-rotate 1.4s linear infinite;
}

.entry-details__player-loading-spinner-track {
  stroke: var(--border-color-primary);
  opacity: 0.25;
}

.entry-details__player-loading-spinner-head {
  stroke: var(--color-primary);
  stroke-dasharray: 80, 200;
  stroke-dashoffset: 0;
  animation: entry-details-spinner-dash 1.4s ease-in-out infinite;
}

@keyframes entry-details-spinner-rotate {
  100% {
    transform: rotate(360deg);
  }
}

@keyframes entry-details-spinner-dash {
  0% {
    stroke-dasharray: 1, 200;
    stroke-dashoffset: 0;
  }
  50% {
    stroke-dasharray: 90, 200;
    stroke-dashoffset: -35px;
  }
  100% {
    stroke-dasharray: 90, 200;
    stroke-dashoffset: -125px;
  }
}
</style>
