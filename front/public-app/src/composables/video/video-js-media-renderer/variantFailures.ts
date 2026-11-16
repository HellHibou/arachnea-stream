import type { VideoJsPlayer } from './types'
import type { VariantController } from './variantController'

/** Maximum active loading time without metadata or playback progress. */
const VARIANT_LOAD_TIMEOUT_MS = 20_000

/**
 * Handles fatal errors and stalled loads before exposing collection exhaustion.
 * @param player Video.js instance sharing one variant controller.
 * @param variants Active collection and bounded candidate selection.
 * @param onExhausted Callback invoked once when no usable variant remains.
 * @param onFallback Retains playback intent before switching the failed source.
 */
export function installVariantFailureHandling(player: VideoJsPlayer, variants: VariantController, onExhausted: () => void, onFallback: (resume: boolean) => void): void {
  let timer: ReturnType<typeof setTimeout> | null = null
  let exhausted = false
  let playbackIntent = false
  let lastPlaybackTime = 0
  /** Cancels the watchdog for the preceding active load. */
  const clear = () => {
    if (timer !== null) clearTimeout(timer)
    timer = null
  }
  /** Ignores events from a source superseded during an asynchronous transition. */
  const isCurrent = () => {
    try {
      return new URL(((player.currentSource() as { src?: string } | null)?.src || player.currentSrc()), window.location.href).href === new URL(variants.source.value.src, window.location.href).href
    } catch { return false }
  }
  /** Advances the collection once; only terminal exhaustion reaches the parent. */
  const fail = () => {
    clear()
    if (!isCurrent() || exhausted) return
    onFallback(playbackIntent)
    if (variants.fail()) {
      player.error(null)
      player.removeClass('vjs-error')
      player.getChild('errorDisplay')?.hide()
    } else {
      exhausted = true
      if (!player.error()) player.error(4)
      onExhausted()
    }
  }
  /** Watches active loading only; autoplay rejection and paused playback do not fail. */
  const start = () => {
    if (timer !== null || !isCurrent()) return
    const expectedSource = variants.source.value
    timer = setTimeout(() => {
      if (expectedSource === variants.source.value) fail()
    }, VARIANT_LOAD_TIMEOUT_MS)
  }
  player.on('error', fail)
  player.on('loadstart', () => { exhausted = false; lastPlaybackTime = player.currentTime() ?? 0; clear(); start() })
  player.on('play', () => { playbackIntent = true; start() })
  player.on('pause', () => { if (!player.error()) playbackIntent = false })
  player.on('waiting', () => { if (!player.paused()) start() })
  player.on('timeupdate', () => {
    const currentTime = player.currentTime() ?? 0
    if (currentTime !== lastPlaybackTime) clear()
    lastPlaybackTime = currentTime
  })
  player.on(['loadedmetadata', 'canplay', 'playing', 'pause', 'ended'], clear)
  player.on('dispose', clear)
}
