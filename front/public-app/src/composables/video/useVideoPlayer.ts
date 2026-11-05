import { useAttrs } from 'vue'
import type { VideoPlayerEmits, VideoPlayerProps } from './playerTypes'
import { usePlayerSurface } from './usePlayerSurface'
import { usePlayerIframe } from './usePlayerIframe'
import { usePlayerVideoAttributes } from './usePlayerVideoAttributes'
import { usePlayerHandlers } from './usePlayerHandlers'

/**
 * Composes the shared video surface, iframe policy, renderer attributes and handlers.
 * @param props Reactive video player configuration.
 * @param emit Parent event emitter.
 * @returns The existing view model consumed by VideoPlayer.vue.
 */
export function useVideoPlayer(props: VideoPlayerProps, emit: VideoPlayerEmits) {
  const surface = usePlayerSurface(props, useAttrs())
  const iframe = usePlayerIframe(props, {
    surface: surface.state,
    isActiveSourceAllowlisted: surface.isActiveSourceAllowlisted,
  })
  const videoAttributes = usePlayerVideoAttributes(props, {
    surface: surface.state,
    sourcePreservingPlayback: surface.sourcePreservingPlayback,
  })
  const handlers = usePlayerHandlers(props, emit, {
    surface: surface.state,
    markSelectionPending: surface.markSelectionPending,
  })

  return { ...surface.state, ...iframe, ...videoAttributes, ...handlers }
}
