import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  shallowRef,
  watch,
  type Ref,
} from 'vue'

import type {
  MediaCardCollectionMode,
  ThumbnailImageFit,
  ThumbnailOrientation,
} from '@/types/media'

/**
 * Options accepted by the media card collection scroll controller.
 */
interface UseMediaCardCollectionScrollOptions {
  /**
   * Reactive reference to the layout currently used by the collection.
   */
  mode: Ref<MediaCardCollectionMode>
  /**
   * Reactive reference to the number of items rendered by the collection.
   */
  itemsLength: Ref<number>
  /**
   * Reactive reference to the thumbnail orientation applied to every media card.
   */
  thumbnailOrientation: Ref<ThumbnailOrientation>
  /**
   * Reactive reference to the poster fit mode applied to every media card.
   */
  thumbnailImageFit: Ref<ThumbnailImageFit>
  /**
   * Reactive reference to the size multiplier applied to media card thumbnails.
   */
  thumbnailSizeMultiplier: Ref<number>
  /**
   * Reactive reference to the scroll viewport element used by the one-line layout.
   */
  viewportRef: Ref<HTMLDivElement | null>
}

/**
 * Owns horizontal overflow state and navigation for the media card collection.
 *
 * @param options Reactive sources used to keep one-line navigation in sync with layout changes.
 * @returns Scroll state and explicit actions consumed by the collection component.
 */
export function mediaCardCollectionScroll(options: UseMediaCardCollectionScrollOptions) {
  /** Whether the collection can be scrolled to the left. */
  const canScrollLeft = shallowRef(false)
  /** Whether the collection can be scrolled to the right. */
  const canScrollRight = shallowRef(false)
  /** Resize observer for the viewport to update scroll state on size changes. */
  let resizeObserver: ResizeObserver | null = null

  /**
   * Indicates whether horizontal navigation controls should be displayed.
   *
   * @returns True when in single-row mode and scrolling is possible in either direction.
   */
  const showScrollControls = computed(() =>
    options.mode.value === 'single-row' && (canScrollLeft.value || canScrollRight.value),
  )

  /**
   * Updates the horizontal scroll button state from the current collection viewport position.
   */
  function updateScrollState(): void {
    const viewport = options.viewportRef.value

    if (!viewport || options.mode.value !== 'single-row') {
      canScrollLeft.value = false
      canScrollRight.value = false
      return
    }

    const maxScrollLeft = Math.max(viewport.scrollWidth - viewport.clientWidth, 0)
    canScrollLeft.value = viewport.scrollLeft > 1
    canScrollRight.value = viewport.scrollLeft < maxScrollLeft - 1
  }

  /**
   * Scrolls the one-line viewport by roughly one visible page.
   *
   * @param direction - Horizontal direction applied to the collection viewport.
   */
  function scrollRow(direction: 'left' | 'right'): void {
    const viewport = options.viewportRef.value

    if (!viewport) {
      return
    }

    const offset = Math.max(viewport.clientWidth * 0.82, 240)
    viewport.scrollBy({
      left: direction === 'right' ? offset : -offset,
      behavior: 'smooth',
    })
  }

  /**
   * Recomputes horizontal navigation state after layout-affecting prop changes.
   */
  async function syncScrollStateAfterLayoutChange(): Promise<void> {
    await nextTick()
    updateScrollState()
  }

  onMounted(() => {
    void nextTick(() => {
      updateScrollState()

      if (!options.viewportRef.value || typeof ResizeObserver === 'undefined') {
        return
      }

      resizeObserver = new ResizeObserver(() => {
        updateScrollState()
      })
      resizeObserver.observe(options.viewportRef.value)
    })
  })

  onBeforeUnmount(() => {
    resizeObserver?.disconnect()
    resizeObserver = null
  })

  watch(
    () => [
      options.mode.value,
      options.itemsLength.value,
      options.thumbnailOrientation.value,
      options.thumbnailImageFit.value,
      options.thumbnailSizeMultiplier.value,
    ],
    () => {
      void syncScrollStateAfterLayoutChange()
    },
    { flush: 'post' },
  )

  return {
    canScrollLeft,
    canScrollRight,
    showScrollControls,
    updateScrollState,
    scrollRow,
  }
}
