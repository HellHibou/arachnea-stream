import { TIMER_OFFSET_LIVE } from '@/composables/video/video-js-media-renderer/constants'
import type { VideoJsPlayer } from '@/composables/video/video-js-media-renderer/types'

/**
 * Checks if the player has a valid, finite duration available.
 *
 * @param player Video.js player to check.
 * @returns `true` when the duration is available and finite.
 */
export function isDurationAvailable(player: VideoJsPlayer): boolean {
  const duration = player.duration()

  return typeof duration === 'number' && Number.isFinite(duration) && duration > 0
}

/**
 * Checks if the current source is a live stream.
 *
 * @param player Video.js player to check.
 * @returns `true` when the duration indicates a live stream.
 */
export function isLiveStream(player: VideoJsPlayer): boolean {
  return player.duration() === Infinity
}

/**
 * Returns the effective seekable range for the current live stream when available.
 *
 * @param player Video.js player currently bound to the renderer.
 * @returns Live seekable range, or `null` when the current source is not seekable.
 */
export function getLiveSeekableRange(player: VideoJsPlayer): { start: number; end: number } | null {
  if (!isLiveStream(player)) {
    return null
  }

  const seekable = player.seekable()

  if (seekable && seekable.length > 0) {
    return {
      start: seekable.start(0),
      end: seekable.end(0),
    }
  }

  return null
}

/**
 * Calculates the display offset from the live edge.
 *
 * @param player Video.js player currently bound to the renderer.
 * @returns Offset in seconds, where values close to zero mean "at live edge".
 */
export function getLiveTimeOffset(player: VideoJsPlayer): number {
  const seekableRange = getLiveSeekableRange(player)

  if (!seekableRange) {
    return 0
  }

  const currentTime = player.currentTime() || 0
  return currentTime - seekableRange.end
}

/**
 * Formats one live offset for the current-time label.
 *
 * @param offsetSeconds Offset from the live edge in seconds.
 * @returns `LIVE` near the live edge, otherwise a negative time delta.
 */
export function formatLiveTimeOffset(offsetSeconds: number): string {
  if (Math.abs(offsetSeconds) <= TIMER_OFFSET_LIVE) {
    return 'LIVE'
  }

  const absSeconds = Math.abs(offsetSeconds)
  const minutes = Math.floor(absSeconds / 60)
  const seconds = Math.floor(absSeconds % 60)

  return `-${minutes}:${seconds.toString().padStart(2, '0')}`
}

/**
 * Synchronizes the CSS classes that indicate duration availability.
 *
 * @param player Video.js player to synchronize.
 */
export function syncDurationAvailabilityState(player: VideoJsPlayer) {
  const playerElement = player.el()

  if (!(playerElement instanceof HTMLElement)) {
    return
  }

  playerElement.classList.toggle(
    'arachnea-duration-unavailable',
    !isDurationAvailable(player),
  )

  playerElement.classList.toggle(
    'arachnea-live-stream',
    isLiveStream(player),
  )
}

/**
 * Seeks to the requested time while keeping live streams inside their seekable range.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param targetTime Target time to seek to.
 */
export function seekInLiveStream(player: VideoJsPlayer, targetTime: number) {
  if (!isLiveStream(player)) {
    player.currentTime(targetTime)
    return
  }

  const seekable = player.seekable()

  if (seekable && seekable.length > 0) {
    const start = seekable.start(0)
    const end = seekable.end(0)
    const clampedTime = Math.max(start, Math.min(end, targetTime))
    player.currentTime(clampedTime)
    return
  }

  player.currentTime(targetTime)
}

/**
 * Updates the custom progress and time display used for live streams.
 *
 * @param player Video.js player currently bound to the renderer.
 */
export function updateLiveProgressBar(player: VideoJsPlayer) {
  if (!isLiveStream(player)) {
    return
  }

  const playerElement = player.el()

  if (!(playerElement instanceof HTMLElement)) {
    return
  }

  const seekableRange = getLiveSeekableRange(player)

  if (!seekableRange) {
    return
  }

  const currentTime = player.currentTime() || 0
  const liveEdge = seekableRange.end
  const bufferStart = seekableRange.start
  const progressRatio = (currentTime - bufferStart) / (liveEdge - bufferStart)
  const clampedRatio = Math.max(0, Math.min(1, progressRatio))

  const playProgress = playerElement.querySelector<HTMLElement>('.vjs-play-progress')

  if (playProgress) {
    playProgress.style.width = `${clampedRatio * 100}%`
  }

  const currentTimeElement = playerElement.querySelector<HTMLElement>('.vjs-current-time')

  if (currentTimeElement) {
    const timeOffset = getLiveTimeOffset(player)
    currentTimeElement.textContent = formatLiveTimeOffset(timeOffset)
  }
}
