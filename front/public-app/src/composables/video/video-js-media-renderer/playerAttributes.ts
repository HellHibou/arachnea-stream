import type { VideoJsPlayer } from '@/composables/video/video-js-media-renderer/types'
import type { VideoJsMediaRendererProps } from '@/composables/video/video-js-media-renderer/types'
import type { ShallowRef } from 'vue'

import { markImageUrlFailed } from '@/composables/media/useFailedImageUrls'

/**
 * Applies the reactive accessibility and autoplay attributes directly to the video element.
 *
 * @param element Native video element managed by Video.js.
 * @param props Component props.
 */
export function syncVideoElementAttributes(
  element: HTMLVideoElement,
  props: VideoJsMediaRendererProps,
): void {
  element.style.setProperty('--vjs-theme-city--primary', '#46a3ff')
  element.style.setProperty('--vjs-theme-city--secondary', '#f7fbff')
  element.defaultMuted = props.muted ?? false
  element.muted = props.muted ?? false
  element.autoplay = props.autoplay ?? false
  element.loop = props.loop ?? false
  element.playsInline = props.playsinline ?? false
  element.preload = props.preload ?? 'metadata'

  if (props.poster) {
    element.poster = props.poster
  } else {
    element.removeAttribute('poster')
  }

  if (props.ariaHidden) {
    element.setAttribute('aria-hidden', 'true')
  } else {
    element.removeAttribute('aria-hidden')
  }

  if (props.videoAriaLabel) {
    element.setAttribute('aria-label', props.videoAriaLabel)
  } else {
    element.removeAttribute('aria-label')
  }

  if (props.tabIndex === null || props.tabIndex === undefined) {
    element.removeAttribute('tabindex')
  } else {
    element.tabIndex = props.tabIndex
  }
}

/**
 * Hides the Video.js poster image when its source fails to load and advances
 * any poster fallback chain.
 *
 * Video.js renders the poster as an `img` inside `.vjs-poster` and does not
 * hide it automatically when the image cannot be fetched. The failed URL is also
 * recorded so page-level poster resolvers skip it like a `null` candidate and
 * fall back to the next available image.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param poster Current poster URL from props.
 * @param markImageUrlFailed Callback recording a failed poster URL.
 */
export function installPosterImageErrorHandler(
  player: VideoJsPlayer,
  poster: string | undefined,
  markImageUrlFailed: (url: string | undefined) => void,
): void {
  const install = () => {
    const playerElement = player.el()
    const posterImage = playerElement?.querySelector<HTMLImageElement>('.vjs-poster img')

    if (!posterImage) {
      return
    }

    if (posterImage.dataset.posterErrorHandlerInstalled === 'true') {
      return
    }
    posterImage.dataset.posterErrorHandlerInstalled = 'true'

    posterImage.addEventListener('error', () => {
      markImageUrlFailed(poster)
      const posterElement = player.el()?.querySelector<HTMLElement>('.vjs-poster')
      posterElement?.setAttribute('hidden', 'true')
      player.poster('')
    })
  }

  // Video.js (re)creates the `img` whenever the poster source changes, so the
  // handler must be (re)attached when the poster-change event surfaces the element.
  install()
  player.on('posterchange', install)
}

/**
 * Applies the host attribute used to suppress the big play button without waiting for a Vue render.
 *
 * @param hostElement Host element ref.
 * @param shouldSuppressBigPlayButton Indicates whether the big play overlay should stay hidden.
 */
export function syncBigPlaySuppressionAttribute(
  hostElement: ShallowRef<HTMLElement | null>,
  shouldSuppressBigPlayButton: boolean,
): void {
  const element = hostElement.value

  if (!element) {
    return
  }

  if (shouldSuppressBigPlayButton) {
    element.setAttribute('data-vjs-initial-loading', 'true')
    return
  }

  element.removeAttribute('data-vjs-initial-loading')
}
