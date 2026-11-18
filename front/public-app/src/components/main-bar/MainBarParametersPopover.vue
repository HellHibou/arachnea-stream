<script setup lang="ts">
import { watch } from 'vue'
import ParametersDialog from '../parameters/ParametersDialog.vue'

/**
 * Compatibility wrapper opening the shared settings dialog from a toolbar.
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
   * Legacy panel identifier retained for existing callers.
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
 * Requests closing the settings dialog.
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
  <ParametersDialog :open="isParametersVisible" @update:open="!$event && closeParameters()" />
</template>
