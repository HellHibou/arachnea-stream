import type { VideoJsPlayer } from '@/composables/video/video-js-media-renderer/types'

/** Class of the player host, reused as the scoped CSS anchor in the PiP document. */
const HOST_CLASS = 'videojs-media-host'

/** Prefix of the attributes Vue injects for scoped styles. */
const SCOPED_ATTRIBUTE_PREFIX = 'data-v-'

/** Event payload emitted by Video.js when the player enters Document PiP. */
type PictureInPictureEnterEvent = {
  /** Window created for the Document PiP session. */
  pipWindow?: Window
}

/**
 * Keeps the player's scoped styles active after Video.js moves the player element
 * out of its host and into a Document Picture-in-Picture window.
 *
 * @param player - Video.js player currently bound to the renderer.
 * @param getHostElement - Callback returning the stable player host element.
 */
export function installPictureInPictureStyles(
  player: VideoJsPlayer,
  getHostElement: () => HTMLDivElement | null,
) {
  let taggedBody: HTMLElement | null = null

  player.on('enterpictureinpicture', (rawEvent: unknown) => {
    const pipWindow = (rawEvent as unknown as PictureInPictureEnterEvent).pipWindow
    const body = pipWindow?.document?.body
    const host = getHostElement()

    if (!body || !host) {
      return
    }

    body.classList.add(HOST_CLASS)
    for (const attribute of Array.from(host.attributes)) {
      if (attribute.name.startsWith(SCOPED_ATTRIBUTE_PREFIX)) {
        body.setAttribute(attribute.name, attribute.value)
      }
    }
    taggedBody = body
  })

  player.on('leavepictureinpicture', () => {
    if (!taggedBody) {
      return
    }

    taggedBody.classList.remove(HOST_CLASS)
    for (const attribute of Array.from(taggedBody.attributes)) {
      if (attribute.name.startsWith(SCOPED_ATTRIBUTE_PREFIX)) {
        taggedBody.removeAttribute(attribute.name)
      }
    }
    taggedBody = null
  })
}
