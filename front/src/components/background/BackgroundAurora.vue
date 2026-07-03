<script setup lang="ts">
/**
 * Props accepted by the fallback aurora background layer.
 */
interface Props {
  /**
   * Enables a decorative motion effect on the aurora layers.
   * @default false
   */
  isAnimated?: boolean
}

/** Component props with applied defaults. */
withDefaults(defineProps<Props>(), {
  isAnimated: false,
})
</script>

<template>
  <div
    class="background__aurora"
    :class="{ 'background__aurora--animated': isAnimated }"
  >
    <div class="background__aurora-layer background__aurora-layer--one" />
    <div class="background__aurora-layer background__aurora-layer--two" />
    <div class="background__aurora-layer background__aurora-layer--three" />
  </div>
</template>

<style scoped>
.background__aurora,
.background__aurora-layer {
  position: absolute;
  inset: 0;
}

.background__aurora {
  overflow: hidden;
}

.background__aurora-layer {
  filter: blur(var(--blur-background-aurora));
  transform-origin: center;
  will-change: transform, opacity;
}

.background__aurora-layer--one {
  background:
    radial-gradient(
      ellipse at 38% 58%,
      rgba(98, 162, 214, 0.4) 0%,
      rgba(98, 162, 214, 0.24) 26%,
      rgba(98, 162, 214, 0.1) 42%,
      transparent 78%
    );
}

.background__aurora-layer--two {
  background:
    radial-gradient(
      ellipse at 64% 58%,
      rgba(92, 176, 198, 0.28) 0%,
      rgba(92, 176, 198, 0.16) 24%,
      rgba(92, 176, 198, 0.06) 42%,
      transparent 78%
    );
}

.background__aurora-layer--three {
  background:
    radial-gradient(
      ellipse at 50% 72%,
      rgba(184, 158, 126, 0.1) 0%,
      rgba(184, 158, 126, 0.05) 22%,
      rgba(184, 158, 126, 0.02) 38%,
      transparent 74%
    );
}

.background__aurora--animated .background__aurora-layer--one {
  animation: background-aurora-one 26s ease-in-out infinite alternate;
}

.background__aurora--animated .background__aurora-layer--two {
  animation: background-aurora-two 30s ease-in-out infinite alternate;
}

.background__aurora--animated .background__aurora-layer--three {
  animation: background-aurora-three 34s ease-in-out infinite alternate;
}

@keyframes background-aurora-one {
  0% {
    transform: translate3d(-2%, -1%, 0) scale(1);
    opacity: 0.82;
  }

  100% {
    transform: translate3d(4%, 2%, 0) scale(1.08);
    opacity: 1;
  }
}

@keyframes background-aurora-two {
  0% {
    transform: translate3d(1%, -2%, 0) scale(1);
    opacity: 0.74;
  }

  100% {
    transform: translate3d(-4%, 3%, 0) scale(1.1);
    opacity: 0.94;
  }
}

@keyframes background-aurora-three {
  0% {
    transform: translate3d(0, 0, 0) scale(1);
    opacity: 0.68;
  }

  100% {
    transform: translate3d(0, -3%, 0) scale(1.06);
    opacity: 0.86;
  }
}

@media (prefers-reduced-motion: reduce) {
  .background__aurora--animated .background__aurora-layer {
    animation: none;
  }
}
</style>
