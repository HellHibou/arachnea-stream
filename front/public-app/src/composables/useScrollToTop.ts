import {
  nextTick,
  onBeforeUnmount,
  onMounted,
  shallowRef,
  toRef,
  toValue,
  type MaybeRefOrGetter,
} from 'vue'

/** The scroll target as an HTMLElement, CSS selector string, or null for the document top. */
type ScrollTarget = HTMLElement | string | null

/** Configuration options for the scroll-to-top composable. */
interface UseScrollToTopOptions {
  /**
   * Scroll offset that toggles the button when no custom visibility rule is provided.
   * @default 300
   */
  threshold?: MaybeRefOrGetter<number>
  /**
   * Element or element id used as the scroll destination.
   * @default null
   */
  target?: MaybeRefOrGetter<ScrollTarget>
  /**
   * Optional custom visibility rule for screens with more specific layout constraints.
   */
  shouldShow?: () => boolean
}

/**
 * Tracks scroll position and exposes a smooth scroll-to-top action.
 *
 * @param options Threshold, target, and optional visibility rule.
 * @returns Button visibility and click handler.
 */
export function useScrollToTop(options: UseScrollToTopOptions = {}) {
  const threshold = toRef(options.threshold ?? 300)
  const showScrollToTop = shallowRef(false)

  /**
   * Scrolls to the configured target or to the top of the document.
   * Uses smooth scrolling behavior.
   */
  function scrollToTop(): void {
    void nextTick(() => {
      window.scrollTo({
        top: getScrollTop(),
        behavior: 'smooth',
      })
    })
  }

  /**
   * Refreshes button visibility from the current scroll position.
   * Uses custom visibility rule if provided, otherwise checks against the threshold.
   */
  function updateScrollToTopVisibility(): void {
    showScrollToTop.value = options.shouldShow
      ? options.shouldShow()
      : window.scrollY > threshold.value
  }

  /**
   * Resolves the target scroll offset.
   *
   * @returns The vertical offset in pixels from the top of the document.
   */
  function getScrollTop(): number {
    const target = options.target ? toValue(options.target) : null

    if (target instanceof HTMLElement) {
      return target.offsetTop
    }

    if (typeof target === 'string') {
      return document.getElementById(target)?.offsetTop ?? 0
    }

    return 0
  }

  onMounted(() => {
    window.addEventListener('scroll', updateScrollToTopVisibility, { passive: true })
    updateScrollToTopVisibility()
  })

  onBeforeUnmount(() => {
    window.removeEventListener('scroll', updateScrollToTopVisibility)
  })

  return {
    showScrollToTop,
    scrollToTop,
  }
}
