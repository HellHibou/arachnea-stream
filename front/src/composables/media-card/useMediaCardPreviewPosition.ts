import { nextTick, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue'

/** Supported popup placements relative to the anchor card. */
export type MediaCardPreviewPlacement = 'right' | 'left' | 'top' | 'bottom'

/** Gap between the popup and the anchor card, in pixels. */
const PLACEMENT_GAP = 10

interface UseMediaCardPreviewPositionOptions {
  /** Reactive reference to the anchor card root element. */
  anchorRef: Ref<HTMLElement | null>
  /** Reactive reference to the popup root element. */
  popupRef: Ref<HTMLElement | null>
  /** Getter indicating whether the popup is currently visible. */
  isOpen: () => boolean
}

interface MediaCardPreviewPositionState {
  /** Horizontal pixel position of the popup (viewport coordinates). */
  left: number | null
  /** Vertical pixel position of the popup (viewport coordinates). */
  top: number | null
  /** Placement side currently applied. */
  placement: MediaCardPreviewPlacement
  /** Whether the popup should be rendered at all. */
  visible: boolean
}

/**
 * Measures the anchor card and the popup, then computes a fixed position
 * that keeps the popup next to the card. Placement prefers the right side
 * and falls back to left, then top, then bottom depending on the available
 * viewport space.
 *
 * @param options Anchor element, popup element and visibility state.
 * @returns Reactive position state and placement used by the popup wrapper.
 */
export function useMediaCardPreviewPosition(options: UseMediaCardPreviewPositionOptions) {
  /** Placements tested in preference order. */
  const placementOrder: MediaCardPreviewPlacement[] = ['right', 'left', 'top', 'bottom']

  /** Current computed position and placement. */
  const state = ref<MediaCardPreviewPositionState>({
    left: null,
    top: null,
    placement: 'right',
    visible: false,
  })

  /** Resize observer watching the popup size changes. */
  const resizeObserver = new ResizeObserver(() => {
    updatePosition()
  })

  /**
   * Finds the first placement that fits the popup inside the viewport.
   *
   * @param anchorRect Bounding rectangle of the anchor card.
   * @param popupRect Bounding rectangle of the popup.
   * @param viewportWidth Current viewport width.
   * @param viewportHeight Current viewport height.
   * @returns Preferred placement, or the right side when nothing fits.
   */
  function findFittingPlacement(
    anchorRect: DOMRect,
    popupRect: DOMRect,
    viewportWidth: number,
    viewportHeight: number,
  ): MediaCardPreviewPlacement {
    const margin = 8

    for (const placement of placementOrder) {
      const top = computeTop(anchorRect, popupRect, placement)
      const left = computeLeft(anchorRect, popupRect, placement)

      if (
        left >= margin &&
        left + popupRect.width <= viewportWidth - margin &&
        top >= margin &&
        top + popupRect.height <= viewportHeight - margin
      ) {
        return placement
      }
    }

    return 'right'
  }

  /**
   * Computes the popup top position relative to the viewport for a placement.
   *
   * @param anchorRect Bounding rectangle of the anchor card.
   * @param popupRect Bounding rectangle of the popup.
   * @param placement Placement side being tested.
   * @returns Vertical pixel position of the popup.
   */
  function computeTop(
    anchorRect: DOMRect,
    popupRect: DOMRect,
    placement: MediaCardPreviewPlacement,
  ): number {
    if (placement === 'top') {
      return anchorRect.top - popupRect.height - PLACEMENT_GAP
    }

    if (placement === 'bottom') {
      return anchorRect.bottom + PLACEMENT_GAP
    }

    // Vertical centering for left/right placements, clamped to the viewport borders.
    const centered = anchorRect.top + (anchorRect.height - popupRect.height) / 2
    return Math.max(8, Math.min(centered, window.innerHeight - popupRect.height - 8))
  }

  /**
   * Computes the popup left position relative to the viewport for a placement.
   *
   * @param anchorRect Bounding rectangle of the anchor card.
   * @param popupRect Bounding rectangle of the popup.
   * @param placement Placement side being tested.
   * @returns Horizontal pixel position of the popup.
   */
  function computeLeft(
    anchorRect: DOMRect,
    popupRect: DOMRect,
    placement: MediaCardPreviewPlacement,
  ): number {
    if (placement === 'right') {
      return anchorRect.right + PLACEMENT_GAP
    }

    if (placement === 'left') {
      return anchorRect.left - popupRect.width - PLACEMENT_GAP
    }

    // Horizontal centering for top/bottom placements, clamped to the viewport borders.
    const centered = anchorRect.left + (anchorRect.width - popupRect.width) / 2
    return Math.max(8, Math.min(centered, window.innerWidth - popupRect.width - 8))
  }

  /**
   * Recomputes the popup position from current DOM measurements.
   */
  function updatePosition(): void {
    const anchor = options.anchorRef.value
    const popup = options.popupRef.value

    if (!anchor || !popup || !options.isOpen()) {
      state.value = { ...state.value, visible: false }
      return
    }

    const anchorRect = anchor.getBoundingClientRect()
    // Force a reflow if the popup has not been laid out yet, so getBoundingClientRect returns accurate dimensions.
    const popupRect = popup.getBoundingClientRect()
    const viewportWidth = window.innerWidth
    const viewportHeight = window.innerHeight

    const placement = findFittingPlacement(anchorRect, popupRect, viewportWidth, viewportHeight)

    state.value = {
      left: Math.round(computeLeft(anchorRect, popupRect, placement)),
      top: Math.round(computeTop(anchorRect, popupRect, placement)),
      placement,
      visible: true,
    }
  }

  /** Listener reusing the position update on window resize. */
  const handleResize = () => {
    updatePosition()
  }

  /** Listener reusing the position update on scroll (capture phase). */
  const handleScroll = () => {
    updatePosition()
  }

  watch(
    options.popupRef,
    (popup) => {
      resizeObserver.disconnect()

      if (popup) {
        resizeObserver.observe(popup)
        updatePosition()
      }
    },
    { immediate: true },
  )

  watch(
    options.isOpen,
    async (isOpen) => {
      if (!isOpen) {
        state.value = { ...state.value, visible: false }
        return
      }

      // Wait for the next tick so the popup is inserted into the DOM before measuring.
      await nextTick()
      updatePosition()
    },
  )

  onMounted(() => {
    window.addEventListener('resize', handleResize)
    document.addEventListener('scroll', handleScroll, true)

    if (options.isOpen()) {
      requestAnimationFrame(() => {
        updatePosition()
      })
    }
  })

  onBeforeUnmount(() => {
    resizeObserver.disconnect()
    window.removeEventListener('resize', handleResize)
    document.removeEventListener('scroll', handleScroll, true)
  })

  return {
    position: state,
  }
}