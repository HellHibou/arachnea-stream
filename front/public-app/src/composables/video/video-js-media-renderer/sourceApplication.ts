import type { ResolvedVideoMediaSource } from '@/services/players'
import type { VideoJsPlayerState } from '@/types/media'
import type {
  VideoJsPlayer,
  VideoJsRemoteTextTrackHandle,
  VideoJsSourceInput,
  VideoJsSpriteThumbnailsPlugin,
} from '@/composables/video/video-js-media-renderer/types'
import type { VideoJsMediaRendererProps } from '@/composables/video/video-js-media-renderer/types'
import type { VideoJsMediaRendererEmits } from '@/composables/video/video-js-media-renderer/types'
import type { StoryboardVttCue } from '@/composables/video/video-js-media-renderer/storyboard'
import type { DashQualityBridge } from '@/composables/video/video-js-media-renderer/dash'
import type { Ref } from 'vue'

import { resolveRetainedQuality } from '@/composables/video/video-js-media-renderer/state'
import { resolveSpriteThumbnailOptions } from '@/composables/video/video-js-media-renderer/storyboard'
import {
  buildDashKeySystemOptions,
  isDashSource,
} from '@/composables/video/video-js-media-renderer/dash'
import { addRemoteSubtitleTracks, clearRemoteSubtitleTracks } from '@/composables/video/video-js-media-renderer/subtitles'
import { restorePersistedTrackPreferences } from '@/composables/video/video-js-media-renderer/state'
import { syncDerivedStoryboardInterval } from '@/composables/video/video-js-media-renderer/storyboard'
import { disableControlBarMenuHoverBehavior } from '@/composables/video/video-js-media-renderer/quality'
import { installSeekOnClick } from '@/composables/video/video-js-media-renderer/controls'
import { syncDisplayedQualityPreference } from '@/composables/video/video-js-media-renderer/quality'

/**
 * Résultat de l'application d'une source sur le player.
 * Regroupe les valeurs d'état modifiées par `applySourceToPlayer`.
 */
export type ApplySourceResult = {
  storyboardLoadId: number
  preferredQuality: string | null
  retainedPlayerState: VideoJsPlayerState | null
  storyboardVttCues: StoryboardVttCue[]
  remoteSubtitleTracks: VideoJsRemoteTextTrackHandle[]
  pendingSourceRestoreCleanup: (() => void) | null
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
export function buildPlayerSource(
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

/**
 * Applies one new media source to the current player while preserving the user state.
 *
 * All mutable state is passed as individual parameters and returned in the result,
 * so the caller remains the single owner of the renderer's state.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param source Resolved source selected for playback.
 * @param playerState State snapshot that should survive the source switch.
 * @param switchTime Playback position captured before the source switch.
 * @param switchAutoplay Indicates whether the preceding source was playing.
 * @param props Component props.
 * @param emit Component emit functions.
 * @param spriteThumbnailsPlugin Sprite thumbnail plugin instance attached to the active player.
 * @param activePlayerValue Current active player value.
 * @param activeDashQualityBridge dash.js quality bridge attached to the active DASH source.
 * @param storyboardLoadId Monotonic storyboard load identifier.
 * @param preferredQuality Preferred quality selected by the user.
 * @param retainedPlayerState Latest persisted player state.
 * @param vttStoryboardGrid Resolved VTT storyboard grid info.
 * @param storyboardVttCues Exact VTT cue geometry for the current hover timestamp.
 * @param remoteSubtitleTracks Remote subtitle tracks explicitly owned by the active renderer.
 * @param clearPendingSourceRestoreCleanup Cleanup for pending source restore operation.
 * @param clearPendingQualitySelectorCleanup Cleanup for pending quality selector override.
 * @param clearPendingDashQualityBridge Cleanup for pending DASH quality bridge installation.
 * @param markVideoInitialLoadStart Marks the video as being in its initial loading phase.
 * @param markVideoInitialLoadComplete Marks the video as having finished initial loading.
 * @param emitCurrentPlayerState Emits the latest persisted Video.js UI state.
 * @param syncExistingPlayerConfiguration Applies reactive configuration to an existing player.
 * @param installDashQualityBridgeForSource Attaches the dash.js quality bridge to a DASH source.
 * @param configureQualityPreferenceSelector Pins the VHS playlist selector to the preferred quality.
 * @returns Updated state values.
 */
export async function applySourceToPlayer(
  player: VideoJsPlayer,
  source: ResolvedVideoMediaSource,
  playerState: VideoJsPlayerState | null,
  switchTime: number | null | undefined,
  switchAutoplay: boolean | undefined,
  props: VideoJsMediaRendererProps,
  emit: VideoJsMediaRendererEmits,
  spriteThumbnailsPlugin: VideoJsSpriteThumbnailsPlugin | null,
  activePlayerValue: VideoJsPlayer | null,
  activeDashQualityBridge: DashQualityBridge | null,
  storyboardLoadId: number,
  preferredQuality: string | null,
  retainedPlayerState: VideoJsPlayerState | null,
  vttStoryboardGrid: Ref<{
    width: number
    height: number
    columns: number
    rows: number
  } | null>,
  storyboardVttCues: StoryboardVttCue[],
  remoteSubtitleTracks: VideoJsRemoteTextTrackHandle[],
  clearPendingSourceRestoreCleanup: () => void,
  clearPendingQualitySelectorCleanup: () => void,
  clearPendingDashQualityBridge: () => void,
  markVideoInitialLoadStart: () => void,
  markVideoInitialLoadComplete: () => void,
  emitCurrentPlayerState: (player: VideoJsPlayer) => void,
  syncExistingPlayerConfiguration: (player: VideoJsPlayer) => void,
  installDashQualityBridgeForSource: (
    player: VideoJsPlayer,
    quality: string | null,
  ) => void,
  configureQualityPreferenceSelector: (
    player: VideoJsPlayer,
    quality: string | null,
    expectedSourceUrl: string,
  ) => void,
  emitVideoMetadataLoaded: (emit: VideoJsMediaRendererEmits) => void,
  attemptAutoplay: (player: VideoJsPlayer, switchAutoplay: boolean | undefined) => void,
): Promise<ApplySourceResult> {
  const nextStoryboardLoadId = storyboardLoadId + 1
  clearPendingSourceRestoreCleanup()
  clearPendingQualitySelectorCleanup()
  clearPendingDashQualityBridge()
  const nextRemoteSubtitleTracks = clearRemoteSubtitleTracks(player, remoteSubtitleTracks)
  markVideoInitialLoadStart()
  const nextPreferredQuality = resolveRetainedQuality(preferredQuality, playerState)
  const nextRetainedPlayerState = playerState
  let nextRetainedQuality = nextPreferredQuality
  vttStoryboardGrid.value = null

  const resolvedStoryboard = await resolveSpriteThumbnailOptions(source)
  const { options: spriteThumbnailOptions, cues } = resolvedStoryboard

  if (nextStoryboardLoadId !== storyboardLoadId + 1 || activePlayerValue !== player) {
    return {
      storyboardLoadId: nextStoryboardLoadId,
      preferredQuality: nextPreferredQuality,
      retainedPlayerState: nextRetainedPlayerState,
      storyboardVttCues: [],
      remoteSubtitleTracks: nextRemoteSubtitleTracks,
      pendingSourceRestoreCleanup: null,
    }
  }

  const nextStoryboardVttCues: StoryboardVttCue[] = cues

  const optWidth = spriteThumbnailOptions.width as number | undefined
  const optHeight = spriteThumbnailOptions.height as number | undefined
  const optColumns = spriteThumbnailOptions.columns as number | undefined
  const optRows = spriteThumbnailOptions.rows as number | undefined
  if (
    typeof optWidth === 'number' && typeof optHeight === 'number' &&
    typeof optColumns === 'number' && typeof optRows === 'number'
  ) {
    vttStoryboardGrid.value = {
      width: optWidth,
      height: optHeight,
      columns: optColumns,
      rows: optRows,
    }
  }

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

    restorePersistedTrackPreferences(player, nextRetainedPlayerState)
    playerStateRestored = true
  }

  const syncRetainedQualityPreference = (treatMissingMenuAsUnavailable = false) => {
    const isDash = isDashSource(source)
    const isQualityPreferenceAvailable = syncDisplayedQualityPreference(
      player,
      nextRetainedQuality,
      { treatMissingMenuAsUnavailable: isDash ? false : treatMissingMenuAsUnavailable },
    )

    if (!isQualityPreferenceAvailable) {
      clearPendingQualitySelectorCleanup()
      nextRetainedQuality = null
      emitCurrentPlayerState(player)
      return
    }

    if (isDash && activeDashQualityBridge) {
      activeDashQualityBridge.applyQualityPreference(nextRetainedQuality)
    }
  }

  const handleLoadedMetadata = () => {
    emitVideoMetadataLoaded(emit)
    syncDerivedStoryboardInterval(player, source, spriteThumbnailsPlugin)
    disableControlBarMenuHoverBehavior(player)
    installSeekOnClick(player, {
      controls: props.controls ?? false,
    })
    restorePlaybackTime()
    restorePlayerUiState()
    syncRetainedQualityPreference()

    if (player.readyState() >= 3) {
      markVideoInitialLoadComplete()
      emitVideoInitialLoadComplete(emit)
      attemptAutoplay(player, switchAutoplay)
    }
  }

  const handleCanPlay = () => {
    emitVideoMetadataLoaded(emit)
    syncDerivedStoryboardInterval(player, source, spriteThumbnailsPlugin)
    restorePlaybackTime()
    restorePlayerUiState()
    syncRetainedQualityPreference(true)
    markVideoInitialLoadComplete()
    emitVideoInitialLoadComplete(emit)
    attemptAutoplay(player, switchAutoplay)
  }

  const handlePlaying = () => {
    markVideoInitialLoadComplete()
    emitVideoInitialLoadComplete(emit)
    clearPendingSourceRestoreCleanup()
  }

  syncExistingPlayerConfiguration(player)
  player.on('loadedmetadata', handleLoadedMetadata)
  player.on('canplay', handleCanPlay)
  player.on('playing', handlePlaying)
  const pendingSourceRestoreCleanup = () => {
    player.off('loadedmetadata', handleLoadedMetadata)
    player.off('canplay', handleCanPlay)
    player.off('playing', handlePlaying)
  }
  player.src(buildPlayerSource(source, spriteThumbnailOptions))
  addRemoteSubtitleTracks(player, source, nextRemoteSubtitleTracks)
  restorePersistedTrackPreferences(player, nextRetainedPlayerState)

  if (isDashSource(source)) {
    installDashQualityBridgeForSource(player, nextRetainedQuality)
  } else {
    configureQualityPreferenceSelector(player, nextRetainedQuality, source.src)
  }

  return {
    storyboardLoadId: nextStoryboardLoadId,
    preferredQuality: nextPreferredQuality,
    retainedPlayerState: nextRetainedPlayerState,
    storyboardVttCues: nextStoryboardVttCues,
    remoteSubtitleTracks: nextRemoteSubtitleTracks,
    pendingSourceRestoreCleanup,
  }
}

function emitVideoInitialLoadComplete(emit: VideoJsMediaRendererEmits): void {
  emit('video-initial-load-complete')
}
