<script setup lang="ts">
/**
 * One option rendered by the reusable segmented control.
 */
interface ParameterSegmentedOption {
  value: string | number | boolean
  label: string
}

/**
 * Props accepted by the reusable segmented parameter control.
 */
interface Props {
  /**
   * Shared radio name used by the native inputs.
   */
  inputName: string
  /**
   * Accessible label applied to the radio group.
   */
  label: string
  /**
   * Currently selected option value.
   */
  modelValue: string | number | boolean
  /**
   * Available options rendered by the control.
   */
  options: ParameterSegmentedOption[]
}

/** Component props without defaults. */
defineProps<Props>()

const emit = defineEmits<{
  /** Emitted when the selected option changes. */
  'update:modelValue': [value: string | number | boolean]
}>()

/**
 * Forwards the selected option to the parent settings section.
 *
 * @param value Newly selected segmented option value.
 */
function handleChange(value: string | number | boolean) {
  emit('update:modelValue', value)
}
</script>

<template>
  <div class="parameters-segmented" role="radiogroup" :aria-label="label">
    <label
      v-for="(option, index) in options"
      :key="index"
      class="parameters-segmented__option"
    >
      <input
        class="parameters-segmented__input"
        type="radio"
        :name="inputName"
        :checked="modelValue === option.value"
        @change="handleChange(option.value)"
      />

      <span class="parameters-segmented__label">
        {{ option.label }}
      </span>
    </label>
  </div>
</template>

<style scoped>
.parameters-segmented {
  display: grid;
  grid-auto-flow: column;
  grid-auto-columns: minmax(0, 1fr);
  gap: 8px;
  width: min(100%, var(--parameters-control-width));
  padding: 6px;
  border-radius: 999px;
  background: var(--bg-transparent);
  box-sizing: border-box;
  box-shadow: none;
}

.parameters-segmented__option {
  position: relative;
}

.parameters-segmented__input {
  position: absolute;
  opacity: 0;
  pointer-events: none;
}

.parameters-segmented__label {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 38px;
  padding: 0 16px;
  border-radius: 999px;
  color: var(--text-primary);
  font-size: 0.95rem;
  font-weight: 600;
  cursor: pointer;
  transition:
    color var(--duration-fast) ease,
    background-color var(--duration-fast) ease,
    box-shadow var(--duration-fast) ease;
}

.parameters-segmented__input:focus-visible + .parameters-segmented__label {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.parameters-segmented__input:checked + .parameters-segmented__label {
  background: var(--bg-accent-primary);
  color: var(--text-primary);
}

@media (max-width: 720px) {
  .parameters-segmented {
    width: 100%;
  }
}
</style>
