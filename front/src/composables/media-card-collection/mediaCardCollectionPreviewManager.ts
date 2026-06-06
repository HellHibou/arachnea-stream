import {
  onBeforeUnmount,
  onMounted,
  shallowRef,
  watch,
  type Ref,
} from 'vue'

import type { MediaCardCollectionMode } from '@/types/media'

/**
 * Options accepted by the media card collection preview manager.
 */
interface UseMediaCardCollectionPreviewManagerOptions {
  /**
   * Layout currently used by the collection.
   */
  mode: Ref<MediaCardCollectionMode>
}

/**
 * Internal controller registered for each mounted media card collection.
 */
interface MediaCardCollectionPreviewController {
  /**
   * Layout currently used by the collection.
   */
  mode: Ref<MediaCardCollectionMode>
  /**
   * Identifier of the currently open preview item.
   */
  openPreviewItemId: Ref<string | null>
  /**
   * Card root elements currently rendered by the collection.
   */
  previewRootElements: Map<string, HTMLElement>
}

const previewControllers = new Set<MediaCardCollectionPreviewController>()

/**
 * Handles outside pointer interactions for every registered collection preview manager.
 *
 * @param event Pointer event captured at the document level.
 */
function handleDocumentPointerDown(event: PointerEvent) {
  const target = event.target as Node | null

  for (const controller of previewControllers) {
    if (controller.mode.value === 'list' || !controller.openPreviewItemId.value) {
      continue
    }

    const rootElement = controller.previewRootElements.get(controller.openPreviewItemId.value)

    if (target && rootElement?.contains(target)) {
      continue
    }

    controller.openPreviewItemId.value = null
  }
}

/**
 * Handles Escape presses for every registered collection preview manager.
 *
 * @param event Keyboard event captured at the window level.
 */
function handleWindowKeyDown(event: KeyboardEvent) {
  if (event.key !== 'Escape') {
    return
  }

  for (const controller of previewControllers) {
    if (controller.mode.value === 'list' || !controller.openPreviewItemId.value) {
      continue
    }

    const rootElement = controller.previewRootElements.get(controller.openPreviewItemId.value)
    controller.openPreviewItemId.value = null
    rootElement?.blur()
  }
}

/**
 * Registers one mounted collection preview controller and installs shared global listeners.
 *
 * @param controller Collection preview controller to register.
 */
function registerPreviewController(controller: MediaCardCollectionPreviewController) {
  if (!previewControllers.size) {
    document.addEventListener('pointerdown', handleDocumentPointerDown)
    window.addEventListener('keydown', handleWindowKeyDown)
  }

  previewControllers.add(controller)
}

/**
 * Unregisters one mounted collection preview controller and removes shared global listeners when unused.
 *
 * @param controller Collection preview controller to unregister.
 */
function unregisterPreviewController(controller: MediaCardCollectionPreviewController) {
  previewControllers.delete(controller)

  if (!previewControllers.size) {
    document.removeEventListener('pointerdown', handleDocumentPointerDown)
    window.removeEventListener('keydown', handleWindowKeyDown)
  }
}

/**
 * Centralizes preview visibility for one media card collection while delegating
 * outside-interaction handling to shared global listeners.
 *
 * @param options Reactive sources used to adapt preview behavior to the active collection layout.
 * @returns Preview state and event handlers consumed by the collection component.
 */
export function mediaCardCollectionPreviewManager(
  options: UseMediaCardCollectionPreviewManagerOptions,
) {
  const openPreviewItemId = shallowRef<string | null>(null)
  const previewRootElements = new Map<string, HTMLElement>()
  const controller: MediaCardCollectionPreviewController = {
    mode: options.mode,
    openPreviewItemId,
    previewRootElements,
  }

  /**
   * Opens the preview for the requested media card item.
   *
   * @param itemId Identifier of the card requesting preview visibility.
   */
  function handlePreviewOpen(itemId: string) {
    if (options.mode.value === 'list') {
      return
    }

    openPreviewItemId.value = itemId
  }

  /**
   * Closes the preview for the requested media card item when it is currently active.
   *
   * @param itemId Identifier of the card requesting preview closure.
   */
  function handlePreviewClose(itemId: string) {
    if (openPreviewItemId.value !== itemId) {
      return
    }

    openPreviewItemId.value = null
  }

  /**
   * Keeps track of the current card root elements so outside clicks only close previews
   * when the interaction happens outside the currently open card.
   *
   * @param payload Card identifier and current root element.
   */
  function handlePreviewRootChange(payload: { itemId: string; element: HTMLElement | null }) {
    if (payload.element) {
      previewRootElements.set(payload.itemId, payload.element)
      return
    }

    previewRootElements.delete(payload.itemId)

    if (openPreviewItemId.value === payload.itemId) {
      openPreviewItemId.value = null
    }
  }

  onMounted(() => {
    registerPreviewController(controller)
  })

  onBeforeUnmount(() => {
    unregisterPreviewController(controller)
    previewRootElements.clear()
  })

  watch(options.mode, (mode) => {
    if (mode === 'list') {
      openPreviewItemId.value = null
    }
  })

  return {
    openPreviewItemId,
    handlePreviewOpen,
    handlePreviewClose,
    handlePreviewRootChange,
  }
}
