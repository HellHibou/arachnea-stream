import type { Ref } from 'vue'

import type {
  VideoJsTextTrackPreference,
  VideoJsTextTrackSettings,
  VideoJsTrackPreference,
} from '@/types/media'

import {
  sanitizeBoolean,
  sanitizeQuality,
  sanitizeTextTrackPreference,
  sanitizeTextTrackSettings,
  sanitizeTrackPreference,
  sanitizeVideoVolume,
} from './sanitizers'
import type { VideoPlayerPreferences } from './types'

/** Reactive refs backing the durable Video.js preferences. */
export interface VideoPlayerPreferenceRefs {
  /** Last selected volume level from 0 to 1. */
  volume: Ref<number>
  /** Whether the player was muted. */
  muted: Ref<boolean>
  /** Last manually selected quality, or `null` for Auto. */
  quality: Ref<string | null>
  /** Last selected audio track. */
  audioTrack: Ref<VideoJsTrackPreference | null>
  /** Last selected subtitle track. */
  textTrack: Ref<VideoJsTextTrackPreference>
  /** Last selected visual subtitle settings. */
  textTrackSettings: Ref<VideoJsTextTrackSettings | null>
}

/** Video.js preference operations exposed by the storage store. */
export interface VideoPlayerPreferencesBridge {
  /** Returns the persisted Video.js preferences. */
  getVideoPlayerPreferences: () => VideoPlayerPreferences
  /** Stores the durable part of one Video.js state snapshot. */
  setVideoPlayerPreferences: (preferences: VideoPlayerPreferences) => void
}

/**
 * Creates the Video.js preference operations bound to the store's reactive refs.
 *
 * @param refs Reactive refs holding the durable player preferences.
 * @returns Preference read/write operations sharing the given refs.
 */
export function createVideoPlayerPreferencesBridge(
  refs: VideoPlayerPreferenceRefs,
): VideoPlayerPreferencesBridge {
    /**
     * Returns the persisted Video.js preferences.
     *
     * @returns Sanitized player preferences used to restore new player instances.
     */
    function getVideoPlayerPreferences(): VideoPlayerPreferences {
        return {
          volume: refs.volume.value,
          muted: refs.muted.value,
          quality: refs.quality.value,
          audioTrack: refs.audioTrack.value,
          textTrack: refs.textTrack.value,
          textTrackSettings: refs.textTrackSettings.value,
        }
    }

    /**
     * Stores the durable part of one Video.js state snapshot.
     *
     * @param playerPreferences Latest durable player preferences.
     */
    function setVideoPlayerPreferences(playerPreferences: VideoPlayerPreferences): void {
        refs.volume.value = sanitizeVideoVolume(playerPreferences.volume)
        refs.muted.value = sanitizeBoolean(playerPreferences.muted, false)

        if (playerPreferences.quality !== null) {
          refs.quality.value = sanitizeQuality(playerPreferences.quality)
        }

        refs.audioTrack.value = sanitizeTrackPreference(playerPreferences.audioTrack)
        refs.textTrack.value = sanitizeTextTrackPreference(playerPreferences.textTrack)
        refs.textTrackSettings.value = sanitizeTextTrackSettings(playerPreferences.textTrackSettings)
    }

  return { getVideoPlayerPreferences, setVideoPlayerPreferences }
}
