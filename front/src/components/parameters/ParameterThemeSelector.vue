<script setup lang="ts">
/**
 * One color swatch option rendered by the theme selector.
 */
interface ThemeOption {
  value: string
  label: string
  color: string
}

/**
 * Props accepted by the theme selector component.
 */
interface Props {
  inputName: string
  modelValue: string
  options: ThemeOption[]
}

defineProps<Props>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()

/**
 * Selects the theme represented by the given swatch.
 *
 * @param value Theme preset key.
 */
function handleSelect(value: string) {
  emit('update:modelValue', value)
}
</script>

<template>
  <div class="theme-selector" role="radiogroup">
    <label
      v-for="option in options"
      :key="option.value"
      class="theme-selector__swatch"
      :class="{ 'theme-selector__swatch--active': modelValue === option.value }"
      :title="option.label"
    >
      <input
        class="theme-selector__input"
        type="radio"
        :name="inputName"
        :value="option.value"
        :checked="modelValue === option.value"
        @change="handleSelect(option.value)"
      />
      <span
        class="theme-selector__color"
        :style="{ backgroundColor: option.color }"
      />
    </label>
  </div>
</template>

<style scoped>
.theme-selector {
  display: inline-flex;
  flex-wrap: wrap;
  gap: 8px;
  width: min(100%, var(--parameters-control-width));
  margin-right: 0px;
}

.theme-selector__swatch {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border-radius: var(--radius);
  cursor: pointer;
  transition:
    box-shadow var(--duration-fast) ease,
    transform var(--duration-fast) ease;
}

.theme-selector__swatch:hover {
  transform: translateY(-1px);
}

.theme-selector__swatch--active {
  box-shadow: 0 0 0 2px var(--color-primary);
}

.theme-selector__input {
  position: absolute;
  opacity: 0;
  pointer-events: none;
}

.theme-selector__input:focus-visible + .theme-selector__color {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.theme-selector__color {
  width: 24px;
  height: 24px;
  border-radius: 6px;
  display: block;
}
</style>