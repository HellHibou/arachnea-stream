<script setup lang="ts">
import { computed } from 'vue'
import { t } from '@/i18n'
import { passwordStrength } from '@/services/password'

/** Current password draft; the indicator never stores or submits it. */
const props = defineProps<{ password: string }>()

const strength = computed(() => passwordStrength(props.password))

/** Maps the coarse level onto a Vuetify progress color. */
const color = computed(() => {
  switch (strength.value.level) {
    case 'veryWeak': return 'error'
    case 'weak':
    case 'fair': return 'warning'
    default: return 'success'
  }
})
</script>

<template>
  <!-- Informative only: the server still enforces the minimum length. -->
  <div v-if="password" class="password-strength" role="status" aria-live="polite">
    <v-progress-linear :model-value="strength.percent" :color="color" height="6" rounded />
    <p class="password-strength__label">
      {{ t('passwordStrength.label') }}: {{ t(`passwordStrength.levels.${strength.level}`) }}
    </p>
  </div>
</template>

<style scoped>
.password-strength {
  margin-block: 4px 12px;
}

.password-strength__label {
  margin-block: 4px 0;
  font-size: 0.875rem;
}
</style>
