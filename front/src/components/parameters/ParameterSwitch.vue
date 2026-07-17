<script setup lang="ts">
/**
 * Props accepted by the reusable parameter switch control.
 */
interface Props {
  /**
   * Unique identifier assigned to the native checkbox.
   */
  inputId: string
  /**
   * Label displayed before the switch handle.
   */
  leadingLabel: string
  /**
   * Label displayed after the switch handle.
   */
  trailingLabel: string
  /**
   * Indicates whether the switch is enabled.
   */
  checked: boolean
  /**
   * Indicates whether the switch should be disabled.
   * @default false
   */
  disabled?: boolean
}

withDefaults(defineProps<Props>(), {
  disabled: false,
})

const emit = defineEmits<{
  /** Emitted when the switch checked state changes. */
  'update:checked': [value: boolean]
}>()

/**
 * Forwards the native checkbox state to the parent settings section.
 *
 * @param event Native checkbox change event.
 */
function handleChange(event: Event) {
  const target = event.target as HTMLInputElement
  emit('update:checked', target.checked)
}
</script>

<template>
  <label
    class="parameters-switch"
    :class="{ 'parameters-switch--disabled': disabled }"
    :for="inputId"
  >
    <span
      class="parameters-switch__text"
      :class="{ 'parameters-switch__text--active': !checked }"
    >
      {{ leadingLabel }}
    </span>

    <input
      :id="inputId"
      class="parameters-switch__input"
      type="checkbox"
      :checked="checked"
      :disabled="disabled"
      @change="handleChange"
    />

    <span class="parameters-switch__control" aria-hidden="true" />

    <span
      class="parameters-switch__text"
      :class="{ 'parameters-switch__text--active': checked }"
    >
      {{ trailingLabel }}
    </span>
  </label>
</template>

<style scoped>
.parameters-switch {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: min(100%, var(--parameters-control-width));
  padding: 6px 10px;
  border: 1px solid var(--border-color-primary);
  border-radius: 999px;
  background: var(--bg-transparent);
  box-sizing: border-box;
  box-shadow: none;
  cursor: pointer;
  user-select: none;
}

.parameters-switch:focus-within {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.parameters-switch--disabled {
  cursor: not-allowed;
}

.parameters-switch__text {
  color: var(--text-disabled);
  font-size: 0.95rem;
  font-weight: 600;
  transition: color var(--duration-fast) ease;
}

.parameters-switch__text--active {
  color: var(--text-primary);
}

.parameters-switch__input {
  position: absolute;
  opacity: 0;
  pointer-events: none;
}

.parameters-switch__control {
  position: relative;
  width: 52px;
  height: 30px;
  border-radius: 999px;
  background: var(--bg-surface);
  transition:
    background-color 180ms ease,
    box-shadow 180ms ease;
}

.parameters-switch__control::after {
  content: '';
  position: absolute;
  top: 3px;
  left: 3px;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: var(--text-primary);
  box-shadow: 0 4px 12px var(--shadow-color);
  transition: transform 180ms ease;
}

.parameters-switch__input:checked + .parameters-switch__control {
  background: var(--bg-accent-primary);
}

.parameters-switch__input:checked + .parameters-switch__control::after {
  transform: translateX(22px);
}

@media (max-width: 720px) {
  .parameters-switch {
    width: 100%;
  }
}
</style>
