import type { ResolvedVideoMediaSource } from '@/services/players'
import type { VideoJsPlayerState } from '@/types/media'
import type { VideoJsPlayer, VideoJsRemoteTextTrackHandle, VideoJsSourceInput, VideoJsMediaRendererProps } from './types'
import type { QualityController } from './qualityController'
import type { StoryboardController } from './storyboardController'
import type { PlayerUiState } from './playerUiState'
import { buildDashKeySystemOptions, isDashSource } from './dash'
import { resolveSpriteThumbnailOptions } from './storyboard'
import { addRemoteSubtitleTracks, clearRemoteSubtitleTracks } from './subtitles'
import { applyPersistedPlayerState, restorePersistedTrackPreferences } from './state'
import type { VideoJsTrackSelectionConfig } from './trackPreferences'
import { disableControlBarMenuHoverBehavior } from './quality'
import { installSeekOnClick } from './controls'

interface SourceControllerOptions {
  props: VideoJsMediaRendererProps
  getPlayer: () => VideoJsPlayer | null
  configurePlayer: (player: VideoJsPlayer) => void
  quality: Pick<QualityController, 'retain' | 'sync' | 'install' | 'reset'>
  storyboard: Pick<StoryboardController, 'configure' | 'syncInterval' | 'reset'>
  ui: Pick<PlayerUiState, 'startSource' | 'metadataLoaded' | 'complete'>
  getTrackSelectionConfig?: () => VideoJsTrackSelectionConfig | null | undefined
}

interface SourceTransition {
  playerState: VideoJsPlayerState | null
  switchTime?: number | null
  switchAutoplay?: boolean
}

/**
 * Owns source transitions, stale-load rejection, temporary listeners and remote tracks.
 * @param options Source props and the narrow collaborating controller interfaces.
 * @returns Source application and cleanup operations for one renderer instance.
 */
export function createSourceController(options: SourceControllerOptions) {
  const { props, getPlayer, configurePlayer, quality, storyboard, ui, getTrackSelectionConfig } = options
  let loadId = 0
  let pendingSourceRestoreCleanup: (() => void) | null = null
  let remoteSubtitleTracks: VideoJsRemoteTextTrackHandle[] = []
  let trackOwner: VideoJsPlayer | null = null

  /** Removes transient listeners from the preceding source transition. */
  function clearSourceListeners() {
    pendingSourceRestoreCleanup?.()
    pendingSourceRestoreCleanup = null
  }

  /** Invalidates pending loads and releases all source-owned resources. */
  function reset() {
    ++loadId
    clearSourceListeners()
    quality.reset()
    if (trackOwner) {
      clearRemoteSubtitleTracks(trackOwner, remoteSubtitleTracks)
    }
    remoteSubtitleTracks = []
    trackOwner = null
    storyboard.reset()
  }

  /**
   * Starts playback when requested by props or the source transition.
   * @param player Player receiving the play request.
   * @param autoplayOverride Whether the preceding source was playing.
   */
  function attemptAutoplay(player: VideoJsPlayer, autoplayOverride?: boolean) {
    if (!props.autoplay && !autoplayOverride) return
    const playPromise = player.play()
    if (playPromise) void playPromise.catch(() => {})
  }

  /**
   * Applies a media source while preserving playback and persisted UI preferences.
   * @param player Current Video.js instance.
   * @param source Resolved media source.
   * @param transition Captured state, optional position and playback intent.
   */
  async function applySource(player: VideoJsPlayer, source: ResolvedVideoMediaSource, transition: SourceTransition) {
    reset()
    const operationId = loadId
    const isCurrent = () => operationId === loadId && getPlayer() === player
    const { playerState, switchTime, switchAutoplay } = transition
    quality.retain(playerState)
    ui.startSource(switchAutoplay)

    const resolved = await resolveSpriteThumbnailOptions(source)
    if (!isCurrent()) return
    storyboard.configure(resolved)
    const spriteThumbnailOptions = resolved.options
    trackOwner = player

    let playbackRestored = false
    let playerStateRestored = false

    const restorePlaybackTime = () => {
      if (playbackRestored) {
        return
      }

      const effectiveInitialPlaybackTime = switchTime ?? props.initialPlaybackTime

      if (
        typeof effectiveInitialPlaybackTime !== 'number' ||
        !Number.isFinite(effectiveInitialPlaybackTime) ||
        effectiveInitialPlaybackTime <= 0
      ) {
        return
      }

      const duration = player.duration()
      const safePlaybackTime =
        typeof duration === 'number' && Number.isFinite(duration) && duration > 0
          ? Math.min(effectiveInitialPlaybackTime, Math.max(duration - 2, 0))
          : effectiveInitialPlaybackTime

      if (safePlaybackTime <= 0) {
        return
      }

      player.currentTime(safePlaybackTime)
      playbackRestored = true
    }

    const restorePlayerUiState = () => {
      if (playerStateRestored) {
        return
      }

      applyPersistedPlayerState(player, playerState, trackSelectionConfig)
      playerStateRestored = true
    }

    const handleLoadedMetadata = () => {
      if (!isCurrent()) {
        return
      }
      ui.metadataLoaded()
      storyboard.syncInterval(player, source)
      disableControlBarMenuHoverBehavior(player)
      installSeekOnClick(player, {
        controls: props.controls ?? false,
      })
      restorePlaybackTime()
      restorePlayerUiState()
      quality.sync(player, source)

      if (player.readyState() >= 3) {
        ui.complete()
        attemptAutoplay(player, switchAutoplay)
      }
    }

    const handleCanPlay = () => {
      if (!isCurrent()) {
        return
      }
      ui.metadataLoaded()
      storyboard.syncInterval(player, source)
      restorePlaybackTime()
      restorePlayerUiState()
      quality.sync(player, source, true)
      ui.complete()
      attemptAutoplay(player, switchAutoplay)
    }

    const handlePlaying = () => {
      if (!isCurrent()) {
        return
      }
      ui.complete()
      clearSourceListeners()
    }

    configurePlayer(player)
    player.on('loadedmetadata', handleLoadedMetadata)
    player.on('canplay', handleCanPlay)
    player.on('playing', handlePlaying)
    pendingSourceRestoreCleanup = () => {
      player.off('loadedmetadata', handleLoadedMetadata)
      player.off('canplay', handleCanPlay)
      player.off('playing', handlePlaying)
    }
    player.src(buildPlayerSource(source, spriteThumbnailOptions))
    addRemoteSubtitleTracks(player, source, remoteSubtitleTracks)
    const trackSelectionConfig = getTrackSelectionConfig?.() ?? undefined
    restorePersistedTrackPreferences(player, playerState, trackSelectionConfig)

    const audioTracks = player.audioTracks?.()
    const textTracks = player.textTracks?.()
    let reapplyTimer: number | null = null
    const scheduleReapply = () => {
      if (reapplyTimer !== null) {
        window.clearTimeout(reapplyTimer)
      }
      reapplyTimer = window.setTimeout(() => {
        if (!isCurrent()) {
          return
        }
        const nextConfig = getTrackSelectionConfig?.() ?? undefined
        restorePersistedTrackPreferences(player, playerState, nextConfig)
      }, 150)
    }
    const handleAddTrack = () => scheduleReapply()
    audioTracks?.addEventListener?.('addtrack', handleAddTrack)
    textTracks?.addEventListener?.('addtrack', handleAddTrack)
    const previousCleanup = pendingSourceRestoreCleanup
    pendingSourceRestoreCleanup = () => {
      previousCleanup?.()
      if (reapplyTimer !== null) {
        window.clearTimeout(reapplyTimer)
      }
      audioTracks?.removeEventListener?.('addtrack', handleAddTrack)
      textTracks?.removeEventListener?.('addtrack', handleAddTrack)
    }

    quality.install(player, source)
  }

  return { applySource, reset }
}

/**
 * Builds the Video.js source descriptor expected by the source handlers.
 *
 * DASH sources carry their Widevine license URL through `keySystemOptions`, which
 * the DASH source handler forwards to dash.js. HLS and progressive sources keep the
 * Video.js native/EME path.
 *
 * @param source Resolved source selected for playback.
 * @param spriteThumbnailOptions Resolved storyboard configuration for the sprite plugin.
 * @returns Video.js source configuration.
 */
function buildPlayerSource(
  source: ResolvedVideoMediaSource,
  spriteThumbnailOptions: Record<string, unknown>,
): VideoJsSourceInput {
  const sourceInput: VideoJsSourceInput = {
    src: source.src,
    spriteThumbnails: spriteThumbnailOptions,
  }

  if (source.mimeType) {
    sourceInput.type = source.mimeType
  }

  if (isDashSource(source)) {
    const keySystemOptions = buildDashKeySystemOptions(source)

    if (keySystemOptions) {
      sourceInput.keySystemOptions = keySystemOptions
    }
  }

  return sourceInput
}
