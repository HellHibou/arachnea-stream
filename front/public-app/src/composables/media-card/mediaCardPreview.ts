import { onBeforeUnmount, ref, watch } from 'vue'

/**
 * Options accepted by the media card preview boundary composable.
 */
interface UseMediaCardPreviewOptions {
  /**
   * Callback invoked when the current card should expose its preview.
   */
  onOpen: () => void
  /**
   * Callback invoked when the current card should hide its preview.
   */
  onClose: () => void
  /**
   * Callback invoked whenever the current card root element changes.
   *
   * @param element - The card root element, or null when unmounted.
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
  /** Reference to the card root element. */
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
  function openPreview(): void {
    options.onOpen()
  }

  /**
   * Closes the hover preview as soon as the pointer leaves the card.
   */
  function closePreview(): void {
    options.onClose()
  }

  /**
   * Keeps the preview open while focus stays inside the current card.
   *
   * @param event - Focus transition emitted by the card container.
   */
  function handleFocusOut(event: FocusEvent): void {
    const nextTarget = event.relatedTarget as Node | null

    if (nextTarget && cardRef.value?.contains(nextTarget)) {
      return
    }

    options.onClose()
  }

  return {
    cardRef,
    openPreview,
    closePreview,
    handleFocusOut,
  }
}
