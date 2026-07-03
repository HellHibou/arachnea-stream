<script setup lang="ts">
import { onBeforeUnmount, onMounted, watch } from 'vue'
import Parameters from '../Parameters.vue'

/**
 * Props accepted by the main toolbar parameters popover.
 */
interface Props {
  /**
   * Indicates whether the parameters panel should be visible.
   * @default false
   */
  isParametersVisible?: boolean
  /**
   * Identifier of the toggle button controlling this panel.
   */
  toggleButtonId: string
  /**
   * Identifier applied to the parameters panel container.
   */
  panelId: string
}

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
  isParametersVisible: false,
})

const emit = defineEmits<{
  /** Emitted when the parameters panel should be closed. */
  'close-parameters': []
}>()

/**
 * Requests closing the parameters popover.
 */
function closeParameters() {
  emit('close-parameters')
}

/**
 * Restores keyboard focus to the toolbar toggle button controlling this popover.
 */
function focusToggleButton() {
  const toggleButton = document.getElementById(props.toggleButtonId)

  if (!(toggleButton instanceof HTMLElement)) {
    return
  }

  toggleButton.focus()
}

/**
 * Closes the popover when Escape is pressed while the panel is visible.
 *
 * @param event Keyboard event captured at the window level.
 */
function handleWindowKeyDown(event: KeyboardEvent) {
  if (!props.isParametersVisible || event.key !== 'Escape') {
    return
  }

  closeParameters()
}

/** Adds keyboard event listener when component mounts. */
onMounted(() => {
  window.addEventListener('keydown', handleWindowKeyDown)
})

/** Removes keyboard event listener when component unmounts. */
onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleWindowKeyDown)
})

/** Restores focus to toggle button when parameters panel is closed. */
watch(
  () => props.isParametersVisible,
  (isParametersVisible, wasParametersVisible) => {
    if (!isParametersVisible && wasParametersVisible) {
      focusToggleButton()
    }
  },
)
</script>

<template>
  <div v-if="isParametersVisible" class="main-bar__popover">
    <div
      class="main-bar__dismiss-layer"
      aria-hidden="true"
      @click="closeParameters"
    />

    <div
      :id="panelId"
      class="main-bar__parameters"
      @click.stop
    >
      <Parameters />
    </div>
  </div>
</template>

<style scoped>
.main-bar__popover {
  position: absolute;
  top: calc(100% + 8px);
  right: 0;
  left: 0;
  z-index: 1;
}

.main-bar__dismiss-layer {
  position: fixed;
  inset: 0;
  border: 0;
  background: var(--bg-transparent);
}

.main-bar__parameters {
  position: relative;
  z-index: 1;
  width: min(100%, 620px);
  max-width: calc(100vw - 32px);
  margin-left: auto;
  padding: 6px 8px 8px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-transparent);
  backdrop-filter: var(--backdrop-filter-strong);
  -webkit-backdrop-filter: var(--backdrop-filter-strong);
  box-shadow:
    var(--shadow-heavy),
    var(--inset-light);
}

@media (max-width: 640px) {
  .main-bar__parameters {
    max-width: calc(100vw - 20px);
    padding: 6px 6px 8px;
  }
}
</style>
