import type { VideoJsPlayer, VideoJsRemoteTextTrackHandle, VideoJsSpriteThumbnailsPlugin } from '@/composables/video/video-js-media-renderer/types'
import type { VideoJsPlayerState } from '@/types/media'
import type { Ref, ShallowRef } from 'vue'

import { clearRemoteSubtitleTracks } from '@/composables/video/video-js-media-renderer/subtitles'

/**
 * Disposes the current Video.js instance before the renderer switches source or unmounts.
 *
 * All mutable state is passed as individual parameters and returned in the result,
 * so the caller remains the single owner of the renderer's state.
 *
 * @param activePlayer Current active Video.js player instance ref.
 * @param remoteSubtitleTracks Remote subtitle tracks explicitly owned by the active renderer.
 * @param clearPendingSourceRestoreCleanup Cleanup for pending source restore operation.
 * @param clearPendingQualitySelectorCleanup Cleanup for pending quality selector override.
 * @param clearPendingDashQualityBridge Cleanup for pending DASH quality bridge installation.
 * @param spriteThumbnailsPlugin Sprite thumbnail plugin instance attached to the active player.
 * @param retainedPlayerState Latest persisted player state.
 * @param isPosterOverlayVisible Whether the poster overlay is currently visible (Ref).
 * @param hasEmittedInitialLoadComplete Whether the initial load complete event has been emitted.
 * @param previewedNavigationDirection Direction of the prev/next control currently previewed (Ref).
 * @returns Updated state values.
 */
export function destroyActivePlayer(
  activePlayer: Ref<VideoJsPlayer | null>,
  remoteSubtitleTracks: VideoJsRemoteTextTrackHandle[],
  clearPendingSourceRestoreCleanup: () => void,
  clearPendingQualitySelectorCleanup: () => void,
  clearPendingDashQualityBridge: () => void,
  spriteThumbnailsPlugin: VideoJsSpriteThumbnailsPlugin | null,
  retainedPlayerState: VideoJsPlayerState | null,
  isPosterOverlayVisible: Ref<boolean>,
  hasEmittedInitialLoadComplete: boolean,
  previewedNavigationDirection: Ref<-1 | 1 | null>,
): {
  remoteSubtitleTracks: VideoJsRemoteTextTrackHandle[]
  spriteThumbnailsPlugin: VideoJsSpriteThumbnailsPlugin | null
  retainedPlayerState: VideoJsPlayerState | null
  isPosterOverlayVisible: boolean
  hasEmittedInitialLoadComplete: boolean
} {
  clearPendingSourceRestoreCleanup()
  clearPendingQualitySelectorCleanup()
  clearPendingDashQualityBridge()

  if (activePlayer.value) {
    clearRemoteSubtitleTracks(activePlayer.value, remoteSubtitleTracks)
  }

  activePlayer.value?.dispose()
  activePlayer.value = null

  return {
    remoteSubtitleTracks: [],
    spriteThumbnailsPlugin: null,
    retainedPlayerState: null,
    isPosterOverlayVisible: false,
    hasEmittedInitialLoadComplete: false,
  }
}

/**
 * Marks the video as being in its initial loading phase.
 *
 * @param isVideoInitialLoading Whether the video is in initial loading state (Ref).
 * @param hasEmittedCurrentSourceMetadata Whether the metadata loaded event has been emitted (Ref).
 * @param hostElement Host element ref.
 * @param syncBigPlaySuppressionAttribute Applies the big play suppression attribute.
 */
export function markVideoInitialLoadStart(
  isVideoInitialLoading: Ref<boolean>,
  hasEmittedCurrentSourceMetadata: Ref<boolean>,
  hostElement: ShallowRef<HTMLElement | null>,
  syncBigPlaySuppressionAttribute: (element: ShallowRef<HTMLElement | null>, value: boolean) => void,
): void {
  isVideoInitialLoading.value = true
  hasEmittedCurrentSourceMetadata.value = false
  syncBigPlaySuppressionAttribute(hostElement, true)
}

/**
 * Marks the video as having finished initial loading.
 *
 * @param isVideoInitialLoading Whether the video is in initial loading state (Ref).
 * @param hostElement Host element ref.
 * @param isExternalLoading Indicates whether loading is handled externally.
 * @param syncBigPlaySuppressionAttribute Applies the big play suppression attribute.
 */
export function markVideoInitialLoadComplete(
  isVideoInitialLoading: Ref<boolean>,
  hostElement: ShallowRef<HTMLElement | null>,
  isExternalLoading: boolean | null | undefined,
  syncBigPlaySuppressionAttribute: (element: ShallowRef<HTMLElement | null>, value: boolean) => void,
): void {
  isVideoInitialLoading.value = false
  syncBigPlaySuppressionAttribute(hostElement, isExternalLoading ?? false)
}
