import { computed, ref } from 'vue'
import { useStorage, type VideoPlayerPreferences } from '@/services/storage'
import type { VideoJsPlayerState } from '@/types/media'
import type { VideoPlayerEmits, VideoPlayerProps } from './playerTypes'
import type { PlayerSurfaceState } from './usePlayerSurface'

interface PlayerHandlersOptions {
  surface: Pick<PlayerSurfaceState, 'isEntryDetailsMode' | 'entryDetailsSurfaceMode' | 'shouldForwardPrimaryVideoEvents'>
  markSelectionPending: () => void
}

/**
 * Owns persisted Video.js preferences and forwards picker/playback events.
 * @param props Reactive player props.
 * @param emit Parent event emitter.
 * @param options Shared surface decisions and the transition-intent operation.
 * @returns Persisted state, picker models and event handlers.
 */
export function usePlayerHandlers(props: VideoPlayerProps, emit: VideoPlayerEmits, options: PlayerHandlersOptions) {
  const { isEntryDetailsMode, entryDetailsSurfaceMode, shouldForwardPrimaryVideoEvents } = options.surface
  const { markSelectionPending } = options

  const storage = useStorage()

  /**
   * Limits stored audio and quality preferences to player surfaces controlled by the user.
   *
   * @returns True when player preferences should be loaded and saved.
   */
  const shouldUseStoredPlayerPreferences = computed(() =>
    isEntryDetailsMode.value || Boolean(props.controls),
  )

  /** Persisted video player state from storage. */
  const persistedVideoPlayerState = ref<VideoJsPlayerState | null>(
    shouldUseStoredPlayerPreferences.value
      ? createInitialVideoPlayerState(storage.getVideoPlayerPreferences())
      : null,
  )

  /**
   * Exposes a stable language key to the native select element.
   *
   * @returns The active language key or the first available language.
   */
  const languageModel = computed(() =>
    props.activeLanguageKey ?? props.availableLanguages?.[0]?.key ?? '',
  )

  /**
   * Exposes a stable player identifier to the native select element.
   *
   * @returns The active player ID or the first available player.
   */
  const playerModel = computed(() => props.activePlayerId ?? props.filteredPlayers?.[0]?.id ?? '')

  /**
   * Stores the latest Video.js UI state so it can be restored after a source switch.
   *
   * @param value - Latest captured player state, or `null` when unavailable.
   */
  const handlePlayerStateUpdate = (value: VideoJsPlayerState | null): void => {
    persistedVideoPlayerState.value = value

    if (value && shouldUseStoredPlayerPreferences.value) {
      storage.setVideoPlayerPreferences(createVideoPlayerPreferences(value))
    }
  }

  /**
   * Stores the newly selected language in the parent controller.
   *
   * @param event - Native select change event.
   */
  const handleLanguageChange = (event: Event): void => {
    const target = event.target as HTMLSelectElement
    const nextLanguageKey = target.value || null

    if (nextLanguageKey !== languageModel.value) {
      markSelectionPending()
    }

    emit('update:active-language-key', nextLanguageKey)
    emit('remember-current-language')
  }

  /**
   * Stores the newly selected player in the parent controller.
   *
   * @param event - Native select change event.
   */
  const handlePlayerChange = (event: Event): void => {
    const target = event.target as HTMLSelectElement
    const nextPlayerId = target.value || null

    if (nextPlayerId !== playerModel.value) {
      markSelectionPending()
    }

    emit('update:active-player-id', nextPlayerId)
    emit('remember-current-player')
  }

  /**
   * Forwards the current Video.js playback position and duration to the entry details controller.
   *
   * @param value - Playback position in seconds, or `null` when it should be cleared.
   * @param duration - Total media duration in seconds, or `null` when unavailable.
   */
  const handlePlaybackProgressUpdate = (value: number | null, duration: number | null): void => {
    emit('update:playback-progress', value, duration)
  }

  /**
   * Forwards the effective playback start notification to the parent controller.
   *
   * @param sourceUrl - The URL of the source that started playback.
   */
  const handlePlaybackStarted = (sourceUrl: string | null): void => {
    emit('playback-started', sourceUrl)
  }

  /**
   * Forwards the playback-ended notification to the parent controller.
   */
  const handlePlaybackEnded = (): void => {
    emit('playback-ended')
  }

  /**
   * Forwards the episode autoplay preference update emitted by the embedded Video.js control bar.
   *
   * @param value - Indicates whether automatic playback of the next episode is enabled.
   */
  const handleEpisodeAutoplayEnabledUpdate = (value: boolean): void => {
    emit('update:is-episode-autoplay-enabled', value)
  }

  /**
   * Forwards the active Video.js playback progress only when the current surface is a primary video.
   *
   * @param value - Playback position in seconds, or `null` when it should be cleared.
   * @param duration - Total media duration in seconds, or `null` when unavailable.
   */
  const handleActiveVideoPlaybackProgressUpdate = (value: number | null, duration: number | null): void => {
    if (!shouldForwardPrimaryVideoEvents.value) {
      return
    }

    handlePlaybackProgressUpdate(value, duration)
  }

  /**
   * Forwards the active Video.js playback-started notification only when it belongs to the primary video.
   *
   * @param sourceUrl - The URL of the source that started playback.
   */
  const handleActiveVideoPlaybackStarted = (sourceUrl: string | null): void => {
    if (!shouldForwardPrimaryVideoEvents.value) {
      return
    }

    handlePlaybackStarted(sourceUrl)
  }

  /**
   * Forwards the active Video.js playback-ended notification only when it belongs to the primary video.
   */
  const handleActiveVideoPlaybackEnded = (): void => {
    if (!shouldForwardPrimaryVideoEvents.value) {
      return
    }

    handlePlaybackEnded()
  }

  /**
   * Forwards the episode autoplay preference update only when the primary media surface is active.
   *
   * @param value - Indicates whether automatic playback of the next episode is enabled.
   */
  const handleActiveVideoEpisodeAutoplayEnabledUpdate = (value: boolean): void => {
    if (entryDetailsSurfaceMode.value !== 'media') {
      return
    }

    handleEpisodeAutoplayEnabledUpdate(value)
  }

  /**
   * Forwards the video navigation event only when the primary media surface is active.
   *
   * @param direction - Navigation direction: -1 for previous, 1 for next.
   */
  const handleActiveVideoVideoNavigation = (direction: -1 | 1): void => {
    if (entryDetailsSurfaceMode.value !== 'media') {
      return
    }

    emit('navigate-video', direction)
  }

  return {
    persistedVideoPlayerState,
    languageModel,
    playerModel,
    handlePlayerStateUpdate,
    handleLanguageChange,
    handlePlayerChange,
    handlePlaybackProgressUpdate,
    handlePlaybackStarted,
    handlePlaybackEnded,
    handleEpisodeAutoplayEnabledUpdate,
    handleActiveVideoPlaybackProgressUpdate,
    handleActiveVideoPlaybackStarted,
    handleActiveVideoPlaybackEnded,
    handleActiveVideoEpisodeAutoplayEnabledUpdate,
    handleActiveVideoVideoNavigation,
  }
}

/**
 * Builds the initial Video.js state restored from durable player preferences.
 *
 * @param preferences Player preferences persisted in local storage.
 * @returns Player state shape expected by the Video.js renderer.
 */
function createInitialVideoPlayerState(
  preferences: VideoPlayerPreferences,
): VideoJsPlayerState {
  return {
    volume: preferences.volume,
    muted: preferences.muted,
    playbackRate: 1,
    quality: preferences.quality,
    audioTrack: preferences.audioTrack,
    textTrack: preferences.textTrack,
    textTrackSettings: preferences.textTrackSettings,
    isFullscreen: false,
    showsRemainingTime: false,
  }
}

/**
 * Extracts only the player state fields that should be persisted between sessions.
 *
 * @param playerState Latest Video.js renderer state.
 * @returns Durable player preferences safe to store locally.
 */
function createVideoPlayerPreferences(playerState: VideoJsPlayerState): VideoPlayerPreferences {
  return {
    volume: playerState.volume,
    muted: playerState.muted,
    quality: playerState.quality,
    audioTrack: playerState.audioTrack,
    textTrack: playerState.textTrack,
    textTrackSettings: playerState.textTrackSettings,
  }
}
