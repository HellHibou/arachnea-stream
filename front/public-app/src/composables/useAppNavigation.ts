import { computed, nextTick, onMounted, onScopeDispose, shallowRef, watch } from 'vue'
import type { FullscreenDocument } from '@/composables/video/video-js-media-renderer/types'

/**
 * Controls responsive shell navigation without depending on content or player instances.
 * @param options Toolbar measurement and focus restoration callbacks.
 * @returns Drawer state, fullscreen visibility and explicit open/close actions.
 */
export function useAppNavigation(options: { toolbarElement: () => HTMLElement | null; restoreFocus: () => void }) {
  const isMobile = shallowRef(false)
  const isOpen = shallowRef(false)
  const isFullscreen = shallowRef(false)
  const toolbarHeight = shallowRef(64)
  const isVisible = computed(() => !isFullscreen.value && (!isMobile.value || isOpen.value))
  let mediaQuery: MediaQueryList | null = null
  let observer: ResizeObserver | null = null

  /** @param restore Whether dismissal should return focus to the toolbar trigger. */
  function close(restore = true): void {
    const wasOpen = isOpen.value
    isOpen.value = false
    if (restore && wasOpen && !isFullscreen.value) void nextTick(options.restoreFocus)
  }
  /** Opens only the temporary drawer; desktop navigation remains permanently available. */
  function toggle(): void { if (isMobile.value && !isFullscreen.value) isOpen.value ? close() : isOpen.value = true }
  /** Synchronizes native/webkit fullscreen and prevents a drawer surviving fullscreen entry. */
  function updateFullscreen(): void {
    const doc = document as FullscreenDocument
    isFullscreen.value = Boolean(doc.fullscreenElement ?? doc.webkitFullscreenElement ?? doc.mozFullScreenElement ?? doc.msFullscreenElement)
    if (isFullscreen.value) close(false)
  }
  /** Updates responsive state without carrying an overlay between layout modes. */
  function updateViewport(): void { isMobile.value = mediaQuery?.matches ?? false; close(false) }
  onMounted(() => {
    mediaQuery = window.matchMedia('(max-width: 1000px)')
    updateViewport()
    mediaQuery.addEventListener('change', updateViewport)
    updateFullscreen()
    for (const event of ['fullscreenchange', 'webkitfullscreenchange', 'mozfullscreenchange', 'MSFullscreenChange']) {
      document.addEventListener(event, updateFullscreen)
    }
    const toolbar = options.toolbarElement()
    if (toolbar) {
      observer = new ResizeObserver(() => { toolbarHeight.value = Math.ceil(toolbar.getBoundingClientRect().height) })
      observer.observe(toolbar)
      toolbarHeight.value = Math.ceil(toolbar.getBoundingClientRect().height)
    }
  })
  watch(isOpen, (open) => { if (isFullscreen.value && open) close(false) })
  onScopeDispose(() => {
    mediaQuery?.removeEventListener('change', updateViewport)
    observer?.disconnect()
    for (const event of ['fullscreenchange', 'webkitfullscreenchange', 'mozfullscreenchange', 'MSFullscreenChange']) {
      document.removeEventListener(event, updateFullscreen)
    }
  })
  return { isMobile, isOpen, isFullscreen, isVisible, toolbarHeight, toggle, close }
}