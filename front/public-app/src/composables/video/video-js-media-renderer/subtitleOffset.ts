import type { VideoJsPlayer, VideoJsTextTrackHandle, VideoJsTextTrackListHandle } from './types'

/** Playback-only subtitle timing control, never included in persisted player state. */
export interface SubtitleOffsetControl {
  /** Returns the current delay in seconds (positive values display subtitles later). */
  getSeconds: () => number
  /** Applies a finite delay in seconds to current and subsequently loaded cues. */
  setSeconds: (seconds: number) => void
  /** Removes the player and track listeners. */
  dispose: () => void
}

/**
 * Keeps subtitle and caption cue times relative to their original timestamps.
 * @param player Player whose subtitle tracks should be shifted.
 * @returns In-memory timing control and its cleanup operation.
 */
export function createSubtitleOffsetControl(player: VideoJsPlayer): SubtitleOffsetControl {
  let seconds = 0
  const originalTimes = new WeakMap<TextTrackCue, { start: number; end: number }>()
  const observedTracks = new Set<VideoJsTextTrackHandle>()
  const tracks = player.textTracks?.() as unknown as VideoJsTextTrackListHandle | undefined

  /** Synchronizes loaded cues, including tracks added by streaming or translation. */
  function syncTracks() {
    if (!tracks) return

    const currentTracks = new Set<VideoJsTextTrackHandle>()
    for (let index = 0; index < tracks.length; index += 1) {
      const track = tracks[index]
      if (!track || (track.kind !== 'subtitles' && track.kind !== 'captions')) continue

      currentTracks.add(track)
      if (!observedTracks.has(track)) {
        observedTracks.add(track)
        track.addEventListener?.('loadeddata', syncTracks)
      }

      let changed = false
      const cues = track.cues
      for (let cueIndex = 0; cues && cueIndex < cues.length; cueIndex += 1) {
        const cue = cues[cueIndex]
        if (!cue) continue
        let original = originalTimes.get(cue)
        if (!original) {
          if (seconds === 0) continue
          original = { start: cue.startTime, end: cue.endTime }
          originalTimes.set(cue, original)
        }
        const start = Math.max(0, original.start + seconds)
        const end = Math.max(0, original.end + seconds)
        if (cue.startTime !== start || cue.endTime !== end) {
          cue.startTime = start
          cue.endTime = end
          changed = true
        }
      }
      if (changed) track.trigger?.('cuechange')
    }
    for (const track of observedTracks) {
      if (currentTracks.has(track)) continue
      track.removeEventListener?.('loadeddata', syncTracks)
      observedTracks.delete(track)
    }
  }

  player.on('timeupdate', syncTracks)
  player.on('loadedmetadata', syncTracks)
  tracks?.addEventListener?.('addtrack', syncTracks)
  tracks?.addEventListener?.('change', syncTracks)
  syncTracks()

  return {
    getSeconds: () => seconds,
    setSeconds: (value) => {
      if (!Number.isFinite(value)) return
      seconds = value
      syncTracks()
    },
    dispose: () => {
      player.off('timeupdate', syncTracks)
      player.off('loadedmetadata', syncTracks)
      tracks?.removeEventListener?.('addtrack', syncTracks)
      tracks?.removeEventListener?.('change', syncTracks)
      for (const track of observedTracks) {
        track.removeEventListener?.('loadeddata', syncTracks)
      }
      observedTracks.clear()
    },
  }
}
