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

/** Reactive player preferences; only volume and mute are persisted on the device. */
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
  /** Returns the current profile and device player preferences. */
  getVideoPlayerPreferences: () => VideoPlayerPreferences
  /** Updates the retained part of one Video.js state snapshot. */
  setVideoPlayerPreferences: (preferences: VideoPlayerPreferences) => void
}

/**
 * Creates the Video.js preference operations bound to the store's reactive refs.
 *
 * @param refs Reactive refs holding profile and device player preferences.
 * @returns Preference read/write operations sharing the given refs.
 */
export function createVideoPlayerPreferencesBridge(
  refs: VideoPlayerPreferenceRefs,
): VideoPlayerPreferencesBridge {
    /**
     * Returns the current profile and device player preferences.
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
     * Updates the retained part of one Video.js state snapshot.
     *
     * @param playerPreferences Latest durable player preferences.
     */
    function setVideoPlayerPreferences(playerPreferences: VideoPlayerPreferences): void {
        refs.volume.value = sanitizeVideoVolume(playerPreferences.volume)
        refs.muted.value = sanitizeBoolean(playerPreferences.muted, false)

        refs.quality.value = sanitizeQuality(playerPreferences.quality)

        const audioTrack = sanitizeTrackPreference(playerPreferences.audioTrack)
        const textTrack = sanitizeTextTrackPreference(playerPreferences.textTrack)
        if (audioTrack && (audioTrack.origin !== 'profile'
          || !refs.audioTrack.value || refs.audioTrack.value.origin === 'profile')) {
          refs.audioTrack.value = audioTrack
        }
        if (textTrack.origin !== 'profile' || refs.textTrack.value.origin === 'profile') {
          refs.textTrack.value = textTrack
        }
        refs.textTrackSettings.value = sanitizeTextTrackSettings(playerPreferences.textTrackSettings)
    }

  return { getVideoPlayerPreferences, setVideoPlayerPreferences }
}
