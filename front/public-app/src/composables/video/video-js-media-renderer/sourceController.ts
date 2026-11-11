import type { ResolvedVideoMediaSource } from '@/services/players'
import type { VideoJsPlayerState } from '@/types/media'
import type { SubtitleTranslationFile } from '@/types/translation'
import type {
  VideoJsPlayer,
  VideoJsRemoteTextTrackHandle,
  VideoJsSourceInput,
  VideoJsMediaRendererProps,
  VideoJsTextTrackHandle,
  VideoJsTextTrackListHandle,
} from './types'
import type { QualityController } from './qualityController'
import type { StoryboardController } from './storyboardController'
import type { PlayerUiState } from './playerUiState'
import { buildDashKeySystemOptions, isDashSource } from './dash'
import { resolveSpriteThumbnailOptions } from './storyboard'
import { addRemoteSubtitleTracks, clearRemoteSubtitleTracks } from './subtitles'
import {
  applyPersistedPlayerState,
  markAppliedTextTrack,
  prepareTrackPreferenceRestore,
  restorePersistedTrackPreferences,
} from './state'
import { applyTextTrackSelection } from './trackPreferences'
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
  onTrackPreferencesApplied?: (player: VideoJsPlayer) => void
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
  const {
    props,
    getPlayer,
    configurePlayer,
    quality,
    storyboard,
    ui,
    getTrackSelectionConfig,
    onTrackPreferencesApplied,
  } = options
  let loadId = 0
  let pendingSourceRestoreCleanup: (() => void) | null = null
  let pendingTrackRestoreCleanup: (() => void) | null = null
  let remoteSubtitleTracks: VideoJsRemoteTextTrackHandle[] = []
  let trackOwner: VideoJsPlayer | null = null
  let translatedSubtitle: AppliedTranslatedSubtitle | null = null
  let isApplyingTranslatedSubtitle = false

  /** Removes transient listeners from the preceding source transition. */
  function clearSourceListeners() {
    pendingSourceRestoreCleanup?.()
    pendingSourceRestoreCleanup = null
  }

  /** Invalidates pending loads and releases all source-owned resources. */
  function reset() {
    ++loadId
    clearSourceListeners()
    pendingTrackRestoreCleanup?.()
    pendingTrackRestoreCleanup = null
    quality.reset()
    removeTranslatedSubtitle(false)
    if (trackOwner) {
      clearRemoteSubtitleTracks(trackOwner, remoteSubtitleTracks)
    }
    remoteSubtitleTracks = []
    trackOwner = null
    storyboard.reset()
  }

  /**
   * Applies one downloaded translation as a renderer-owned remote WebVTT track.
   *
   * The active video source is never reapplied, so the current playback position
   * is preserved. The previously selected human track is restored when the
   * translated track is cleared.
   *
   * @param file Complete WebVTT document and display metadata.
   * @param origin Origin to preserve when player state is captured.
   * @returns True when the active player accepted the track.
   */
  function applyTranslatedSubtitle(
    file: SubtitleTranslationFile,
    origin: 'manual' | 'profile' = 'profile',
  ): boolean {
    const player = getPlayer()
    if (!player) {
      return false
    }

    const previousTrack = translatedSubtitle?.previousTrack ??
      findShowingDisplayTextTrack(player)
    removeTranslatedSubtitle(false)

    const objectUrl = URL.createObjectURL(file.blob)
    isApplyingTranslatedSubtitle = true
    let addedTrack: VideoJsRemoteTextTrackHandle | undefined
    try {
      addedTrack = player.addRemoteTextTrack?.(
        {
          kind: 'subtitles',
          src: objectUrl,
          label: file.label,
          srclang: file.lang,
        },
        true,
      ) as VideoJsRemoteTextTrackHandle | undefined
    } finally {
      isApplyingTranslatedSubtitle = false
    }
    const track = addedTrack

    if (!track) {
      URL.revokeObjectURL(objectUrl)
      return false
    }

    track.track.autoTranslate = true
    remoteSubtitleTracks.push(track)
    translatedSubtitle = {
      owner: player,
      track,
      objectUrl,
      previousTrack,
    }
    trackOwner = player
    applyTextTrackSelection(player, track.track)
    markAppliedTextTrack(player, track.track, origin)
    return true
  }

  /** Removes the translated track and restores the subtitle choice it replaced. */
  function clearTranslatedSubtitle(): void {
    removeTranslatedSubtitle(true)
  }

  /** Removes the translated track without restoring the replaced subtitle choice. */
  function discardTranslatedSubtitle(): void {
    removeTranslatedSubtitle(false)
  }

  /**
   * Removes the translated track, releasing its Blob URL exactly once.
   *
   * @param restoreSelection Whether the replaced human track should be selected again.
   */
  function removeTranslatedSubtitle(restoreSelection: boolean): void {
    const applied = translatedSubtitle
    if (!applied) {
      return
    }

    translatedSubtitle = null
    applied.owner.removeRemoteTextTrack?.(applied.track)
    remoteSubtitleTracks = remoteSubtitleTracks.filter((track) => track !== applied.track)
    URL.revokeObjectURL(applied.objectUrl)

    if (
      restoreSelection &&
      applied.previousTrack &&
      hasTextTrack(applied.owner, applied.previousTrack)
    ) {
      applyTextTrackSelection(applied.owner, applied.previousTrack)
    }
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
    if (getTrackSelectionConfig?.()) prepareTrackPreferenceRestore(player, transition.playerState)
    reset()
    const operationId = loadId
    const isCurrent = () => operationId === loadId && getPlayer() === player
    const { playerState, switchTime, switchAutoplay } = transition
    quality.retain(playerState)
    ui.startSource(switchAutoplay)

    const resolved = await resolveSpriteThumbnailOptions(source)
    if (!isCurrent()) return
    const trackSelectionConfig = getTrackSelectionConfig?.() ?? undefined
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

      applyPersistedPlayerState(player, playerState, getTrackSelectionConfig?.() ?? undefined)
      playerStateRestored = true
      onTrackPreferencesApplied?.(player)
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
    if (trackSelectionConfig) prepareTrackPreferenceRestore(player, playerState)
    player.src(buildPlayerSource(source, spriteThumbnailOptions))
    addRemoteSubtitleTracks(player, source, remoteSubtitleTracks)
    restorePersistedTrackPreferences(player, playerState, trackSelectionConfig)
    onTrackPreferencesApplied?.(player)

    const audioTracks = player.audioTracks?.()
    const textTracks = player.textTracks?.()
    let reapplyTimer: number | null = null
    const scheduleReapply = () => {
      if (getTrackSelectionConfig?.()) prepareTrackPreferenceRestore(player, playerState)
      if (reapplyTimer !== null) {
        window.clearTimeout(reapplyTimer)
      }
      reapplyTimer = window.setTimeout(() => {
        reapplyTimer = null
        if (!isCurrent()) {
          return
        }
        const nextConfig = getTrackSelectionConfig?.() ?? undefined
        restorePersistedTrackPreferences(player, playerState, nextConfig)
        onTrackPreferencesApplied?.(player)
      }, 150)
    }
    const handleAddTrack = (event: Event) => {
      const eventTrack = (event as Event & { track?: VideoJsTextTrackHandle }).track
      if (isApplyingTranslatedSubtitle || eventTrack?.autoTranslate === true) {
        return
      }
      scheduleReapply()
    }
    audioTracks?.addEventListener?.('addtrack', handleAddTrack)
    textTracks?.addEventListener?.('addtrack', handleAddTrack)
    pendingTrackRestoreCleanup = () => {
      if (reapplyTimer !== null) {
        window.clearTimeout(reapplyTimer)
      }
      audioTracks?.removeEventListener?.('addtrack', handleAddTrack)
      textTracks?.removeEventListener?.('addtrack', handleAddTrack)
    }

    quality.install(player, source)
  }

  return {
    applySource,
    reset,
    applyTranslatedSubtitle,
    clearTranslatedSubtitle,
    discardTranslatedSubtitle,
  }
}

/** One translated track owned by the source controller. */
interface AppliedTranslatedSubtitle {
  /** Player that received the remote track. */
  owner: VideoJsPlayer
  /** Remote text track handle returned by Video.js. */
  track: VideoJsRemoteTextTrackHandle
  /** Temporary Blob URL that must be revoked with the track. */
  objectUrl: string
  /** Human track selected before the translated track was applied. */
  previousTrack: VideoJsTextTrackHandle | null
}

/**
 * Finds the displayable track currently shown by the player.
 *
 * @param player Active Video.js player.
 * @returns Showing subtitle or caption track, or `null` when subtitles are disabled.
 */
function findShowingDisplayTextTrack(
  player: VideoJsPlayer,
): VideoJsTextTrackHandle | null {
  const tracks = (player.textTracks?.() as unknown as VideoJsTextTrackListHandle | undefined) ?? null
  if (!tracks) {
    return null
  }

  for (let index = 0; index < tracks.length; index += 1) {
    const track = tracks[index]
    if (
      track &&
      (track.kind === 'subtitles' || track.kind === 'captions') &&
      track.mode === 'showing'
    ) {
      return track
    }
  }

  return null
}

/**
 * Checks whether a track is still exposed by the player.
 *
 * @param player Active Video.js player.
 * @param expected Track captured before the translated track was applied.
 * @returns True when the same track handle is still present.
 */
function hasTextTrack(
  player: VideoJsPlayer,
  expected: VideoJsTextTrackHandle,
): boolean {
  const tracks = (player.textTracks?.() as unknown as VideoJsTextTrackListHandle | undefined) ?? null
  if (!tracks) {
    return false
  }

  for (let index = 0; index < tracks.length; index += 1) {
    if (tracks[index] === expected) {
      return true
    }
  }

  return false
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
