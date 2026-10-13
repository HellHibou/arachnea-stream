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
  const desktopVisible = shallowRef(true)
  const isFullscreen = shallowRef(false)
  const toolbarHeight = shallowRef(64)
  const isVisible = computed(() => !isFullscreen.value && (isMobile.value ? isOpen.value : desktopVisible.value))
  let mediaQuery: MediaQueryList | null = null
  let observer: ResizeObserver | null = null

  /** @param restore Whether dismissal should return focus to the toolbar trigger. */
  function close(restore = true): void {
    const wasOpen = isOpen.value
    isOpen.value = false
    if (restore && wasOpen && !isFullscreen.value) void nextTick(options.restoreFocus)
  }
  /** Toggles the mobile drawer or persisted desktop navigation visibility. */
  function toggle(): void {
    if (isFullscreen.value) return
    if (isMobile.value) isOpen.value ? close() : isOpen.value = true
    else {
      desktopVisible.value = !desktopVisible.value
      try { localStorage.setItem('arachnea.navigation.visible', JSON.stringify(desktopVisible.value)) } catch { /* Navigation remains usable without storage. */ }
    }
  }
  /** Restores desktop visibility without persisting temporary mobile drawer closures. */
  function readVisibility(): void {
    try { desktopVisible.value = localStorage.getItem('arachnea.navigation.visible') !== 'false' } catch { desktopVisible.value = true }
  }
  /** @param event Shared preference update from another browser view. */
  function onStorage(event: StorageEvent): void { if (event.key === 'arachnea.navigation.visible' || event.key === null) readVisibility() }
  /** Synchronizes native/webkit fullscreen and prevents a drawer surviving fullscreen entry. */
  function updateFullscreen(): void {
    const doc = document as FullscreenDocument
    isFullscreen.value = Boolean(doc.fullscreenElement ?? doc.webkitFullscreenElement ?? doc.mozFullScreenElement ?? doc.msFullscreenElement)
    if (isFullscreen.value) close(false)
  }
  /** Updates responsive state without carrying an overlay between layout modes. */
  function updateViewport(): void { isMobile.value = mediaQuery?.matches ?? false; close(false) }
  onMounted(() => {
    readVisibility()
    window.addEventListener("storage", onStorage)
    window.addEventListener("focus", readVisibility)
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
    window.removeEventListener("storage", onStorage)
    window.removeEventListener("focus", readVisibility)
    mediaQuery?.removeEventListener('change', updateViewport)
    observer?.disconnect()
    for (const event of ['fullscreenchange', 'webkitfullscreenchange', 'mozfullscreenchange', 'MSFullscreenChange']) {
      document.removeEventListener(event, updateFullscreen)
    }
  })
  return { isMobile, isOpen, isFullscreen, isVisible, toolbarHeight, toggle, close }
}