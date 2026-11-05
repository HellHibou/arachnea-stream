import { computed, shallowRef } from 'vue'
import type { UseVideoJsMediaRendererOptions } from './types'
import { syncBigPlaySuppressionAttribute } from './playerAttributes'

/**
 * Owns loading, metadata/completion guards and poster/navigation preview state.
 * @param options Reactive renderer props, emit function and template refs.
 * @returns UI state and operations for source transitions and player disposal.
 */
export function createPlayerUiState(options: UseVideoJsMediaRendererOptions) {
  const { props, emit, hostElement, videoElement } = options
  const isPosterOverlayVisible = shallowRef(false)
  const isVideoInitialLoading = shallowRef(true)
  const previewedNavigationDirection = shallowRef<-1 | 1 | null>(null)
  const shouldRenderPosterOverlay = computed(() => Boolean(props.overlayLogoUrl))
  let hasEmittedInitialLoadComplete = false
  let hasEmittedCurrentSourceMetadata = false

  /** Synchronizes the big play button with internal and external loading. */
  function syncLoading() {
    syncBigPlaySuppressionAttribute(hostElement, isVideoInitialLoading.value || (props.isExternalLoading ?? false))
  }

  /**
   * Starts loading a source and resets only source-scoped metadata state.
   * @param autoplayOverride Whether playback should continue from the preceding source.
   */
  function startSource(autoplayOverride = false) {
    isVideoInitialLoading.value = true
    hasEmittedCurrentSourceMetadata = false
    isPosterOverlayVisible.value = shouldRenderPosterOverlay.value && !(props.autoplay || autoplayOverride)
    syncLoading()
  }

  /** Completes loading and emits the initial completion once per player instance. */
  function complete() {
    isVideoInitialLoading.value = false
    syncLoading()
    if (!hasEmittedInitialLoadComplete) {
      hasEmittedInitialLoadComplete = true
      emit('video-initial-load-complete')
    }
  }

  /** Emits natural video dimensions once per source when metadata is available. */
  function metadataLoaded() {
    if (hasEmittedCurrentSourceMetadata) return
    const width = videoElement.value?.videoWidth ?? 0
    const height = videoElement.value?.videoHeight ?? 0
    if (width <= 0 || height <= 0) return
    emit('video-metadata-loaded', { width, height, aspectRatio: width / height })
    hasEmittedCurrentSourceMetadata = true
  }

  /** Hides the logo overlay when playback starts. */
  function hidePoster() {
    isPosterOverlayVisible.value = false
  }

  /**
   * Selects the adjacent episode preview shown by the player chrome.
   * @param direction Previewed direction, or null when no control is previewed.
   */
  function previewNavigation(direction: -1 | 1 | null) {
    previewedNavigationDirection.value = direction
  }

  /** Resets player-scoped UI state after disposal. */
  function reset() {
    isPosterOverlayVisible.value = false
    hasEmittedInitialLoadComplete = false
    hasEmittedCurrentSourceMetadata = false
    previewedNavigationDirection.value = null
  }

  return {
    isPosterOverlayVisible, isVideoInitialLoading, shouldRenderPosterOverlay,
    previewedNavigationDirection, startSource, complete, metadataLoaded,
    hidePoster, previewNavigation, syncLoading, reset,
  }
}

export type PlayerUiState = ReturnType<typeof createPlayerUiState>
