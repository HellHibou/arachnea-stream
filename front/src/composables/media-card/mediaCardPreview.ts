import { onBeforeUnmount, ref, watch } from 'vue'

/**
 * Options accepted by the media card preview boundary composable.
 */
interface UseMediaCardPreviewOptions {
  /**
   * Called when the current card should expose its preview.
   */
  onOpen: () => void
  /**
   * Called when the current card should hide its preview.
   */
  onClose: () => void
  /**
   * Called whenever the current card root element changes.
   */
  onRootChange?: (element: HTMLElement | null) => void
}

/**
 * Handles the transient preview state for one interactive media card.
 *
 * @param options Preview lifecycle callbacks forwarded to the parent collection controller.
 * @returns Preview state and DOM event handlers bound to the card root.
 */
export function mediaCardPreview(options: UseMediaCardPreviewOptions) {
  const cardRef = ref<HTMLElement | null>(null)

  watch(cardRef, (element) => {
    options.onRootChange?.(element)
  }, { immediate: true })

  onBeforeUnmount(() => {
    options.onRootChange?.(null)
  })

  /**
   * Opens the hover and focus preview for the current card.
   */
  function openPreview() {
    options.onOpen()
  }

  /**
   * Closes the hover and focus preview for the current card.
   */
  function closePreview() {
    options.onClose()
  }

  /**
   * Keeps the preview open while focus stays inside the current card.
   *
   * @param event Focus transition emitted by the card container.
   */
  function handleFocusOut(event: FocusEvent) {
    const nextTarget = event.relatedTarget as Node | null

    if (nextTarget && cardRef.value?.contains(nextTarget)) {
      return
    }

    closePreview()
  }

  return {
    cardRef,
    openPreview,
    closePreview,
    handleFocusOut,
  }
}
