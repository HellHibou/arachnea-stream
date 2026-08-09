<script setup lang="ts">
/**
 * Props accepted by the reusable slider parameter control.
 */
interface Props {
  /**
   * Accessible label applied to the slider input.
   */
  label: string
  /**
   * Currently selected value.
   */
  modelValue: number
  /**
   * Minimum value accepted by the slider.
   */
  min: number
  /**
   * Maximum value accepted by the slider.
   */
  max: number
  /**
   * Increment step applied to the slider.
   */
  step: number
  /**
   * Optional suffix displayed next to the current value.
   */
  valueSuffix?: string
}

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
  valueSuffix: '',
})

const emit = defineEmits<{
  /** Emitted when the slider value changes. */
  'update:modelValue': [value: number]
}>()

/**
 * Forwards the slider value to the parent settings section.
 *
 * @param event Input event fired by the range input.
 */
function handleInput(event: Event) {
  const target = event.target as HTMLInputElement
  emit('update:modelValue', Number(target.value))
}
</script>

<template>
  <div class="parameters-slider">
    <input
      class="parameters-slider__input"
      type="range"
      :min="min"
      :max="max"
      :step="step"
      :value="modelValue"
      :aria-label="label"
      @input="handleInput"
    />
  </div>
</template>

<style scoped>
.parameters-slider {
  display: grid;
  gap: 8px;
  width: min(100%, var(--parameters-control-width));
  padding-top: 10px;
  padding-bottom: 10px;
}

.parameters-slider__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.parameters-slider__label {
  color: var(--text-primary);
  font-size: 0.95rem;
  font-weight: 600;
}

.parameters-slider__value {
  color: var(--text-secondary);
  font-size: 0.9rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.parameters-slider__input {
  width: 100%;
  height: 4px;
  accent-color: var(--color-primary);
  cursor: pointer;
  -webkit-appearance: none;
  appearance: none;
  background: transparent;
}

/* ===== Chrome / Safari / Edge ===== */
.parameters-slider__input::-webkit-slider-runnable-track {
  height: 4px;
  border-radius: 2px;
  background: color-mix(in srgb, var(--text-disabled) 50%, transparent);
}

.parameters-slider__input::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 16px;
  height: 16px;
  margin-top: -6px;
  border: none;
  border-radius: 50%;
  background: var(--color-primary);
  box-shadow: none;
  cursor: pointer;
}

/* ===== Firefox ===== */
.parameters-slider__input::-moz-range-track {
  height: 8px;
  border-radius: 4px;
  background: color-mix(in srgb, var(--text-disabled) 50%, transparent);
  border: none;
}

.parameters-slider__input::-moz-range-progress {
  height: 8px;
  border-radius: 4px;
  background: var(--color-primary);
}

.parameters-slider__input::-moz-range-thumb {
  width: 20px;
  height: 20px;
  border: none;
  border-radius: 50%;
  background: var(--bg-accent-marker-primary);
  box-shadow: none;
  cursor: pointer;
}

@media (max-width: 720px) {
  .parameters-slider {
    width: 100%;
  }
}
</style>