import type {
  FullscreenDocument,
  FullscreenHostElement,
  PromiseLikeWithCatch,
  VideoJsPlayer,
} from '@/composables/video/video-js-media-renderer/types'

function getFullscreenElement(): Element | null {
  const fullscreenDocument = document as FullscreenDocument

  return fullscreenDocument.fullscreenElement ??
    fullscreenDocument.webkitFullscreenElement ??
    fullscreenDocument.mozFullScreenElement ??
    fullscreenDocument.msFullscreenElement ??
    null
}

function requestHostFullscreen(element: FullscreenHostElement): Promise<unknown> | unknown {
  if (typeof element.requestFullscreen === 'function') {
    return element.requestFullscreen()
  }

  if (typeof element.webkitRequestFullscreen === 'function') {
    return element.webkitRequestFullscreen()
  }

  if (typeof element.msRequestFullscreen === 'function') {
    return element.msRequestFullscreen()
  }

  return undefined
}

function exitDocumentFullscreen(): Promise<unknown> | unknown {
  const fullscreenDocument = document as FullscreenDocument

  if (typeof fullscreenDocument.exitFullscreen === 'function') {
    return fullscreenDocument.exitFullscreen()
  }

  if (typeof fullscreenDocument.webkitExitFullscreen === 'function') {
    return fullscreenDocument.webkitExitFullscreen()
  }

  if (typeof fullscreenDocument.msExitFullscreen === 'function') {
    return fullscreenDocument.msExitFullscreen()
  }

  return undefined
}

function isPromiseLikeWithCatch<T = unknown>(value: unknown): value is PromiseLikeWithCatch<T> {
  return typeof value === 'object' &&
    value !== null &&
    'catch' in value &&
    typeof value.catch === 'function'
}

function syncPlayerFullscreenState(
  player: VideoJsPlayer,
  getHostElement: () => HTMLDivElement | null,
) {
  const host = getHostElement()

  if (!host) {
    return
  }

  const fullscreenElement = getFullscreenElement()
  const nextIsFullscreen = fullscreenElement === host
  const previousIsFullscreen = player.isFullscreen()

  player.isFullscreen(nextIsFullscreen)

  if (previousIsFullscreen !== nextIsFullscreen) {
    player.trigger('fullscreenchange')
  }
}

/**
 * Routes fullscreen requests through the stable wrapper instead of the mutable Video.js element.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param getHostElement Callback returning the stable fullscreen host element.
 */
export function installStableFullscreenBridge(
  player: VideoJsPlayer,
  getHostElement: () => HTMLDivElement | null,
) {
  const originalDocumentFullscreenChange = player.documentFullscreenChange_?.bind(player)
  const originalRequestFullscreen = player.requestFullscreen.bind(player)
  const originalExitFullscreen = player.exitFullscreen.bind(player)

  player.documentFullscreenChange_ = (event: Event) => {
    if (!getHostElement()) {
      originalDocumentFullscreenChange?.(event)
      return
    }

    syncPlayerFullscreenState(player, getHostElement)
  }

  player.requestFullscreen = (fullscreenOptions?: unknown) => {
    const host = getHostElement()

    if (!host) {
      return originalRequestFullscreen(fullscreenOptions)
    }

    if (player.isInPictureInPicture?.()) {
      const exitPictureInPictureResult = player.exitPictureInPicture?.()

      if (isPromiseLikeWithCatch(exitPictureInPictureResult)) {
        void exitPictureInPictureResult.catch(() => {})
      }
    }

    const fullscreenRequest = requestHostFullscreen(host)

    if (fullscreenRequest === undefined) {
      return originalRequestFullscreen(fullscreenOptions)
    }

    return Promise.resolve(fullscreenRequest).catch((error: unknown) => {
      player.trigger('fullscreenerror', error)
      throw error
    })
  }

  player.exitFullscreen = () => {
    const host = getHostElement()

    if (!host || getFullscreenElement() !== host) {
      return originalExitFullscreen()
    }

    const fullscreenExit = exitDocumentFullscreen()

    if (fullscreenExit === undefined) {
      return originalExitFullscreen()
    }

    return Promise.resolve(fullscreenExit).catch((error: unknown) => {
      player.trigger('fullscreenerror', error)
      throw error
    })
  }
}
