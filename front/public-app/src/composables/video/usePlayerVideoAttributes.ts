import { computed, type Ref } from 'vue'
import type { ResolvedVideoMediaSource } from '@/services/players'
import { t } from '@/i18n'
import type { VideoPlayerProps } from './playerTypes'
import type { PlayerSurfaceState } from './usePlayerSurface'

interface PlayerVideoAttributesOptions {
  surface: Pick<PlayerSurfaceState, 'isEntryDetailsMode' | 'entryDetailsSurfaceMode' | 'activeSurfaceSource' | 'detailTitle'>
  sourcePreservingPlayback: Readonly<Ref<string | null>>
}

/**
 * Derives Video.js attributes and navigation settings from the selected surface.
 * @param props Reactive player props.
 * @param options Shared surface refs and the URL whose playback should be preserved.
 * @returns Reactive attributes consumed by the unified Video.js renderer.
 */
export function usePlayerVideoAttributes(props: VideoPlayerProps, options: PlayerVideoAttributesOptions) {
  const { isEntryDetailsMode, entryDetailsSurfaceMode, activeSurfaceSource, detailTitle } = options.surface
  const { sourcePreservingPlayback } = options

  /**
   * Exposes the Video.js variant of the current surface source when selected.
   *
   * @returns The video media source or null.
   */
  const activeVideoSource = computed<ResolvedVideoMediaSource | null>(() =>
    activeSurfaceSource.value?.renderer === 'video'
      ? activeSurfaceSource.value
      : null,
  )

  /**
   * Exposes the accessibility label applied to the unified Video.js renderer.
   *
   * @returns The appropriate aria label based on mode and surface.
   */
  const activeVideoAriaLabel = computed(() => {
    if (!isEntryDetailsMode.value) {
      return props.videoAriaLabel
    }

    if (detailTitle.value) {
      return detailTitle.value
    }

    if (entryDetailsSurfaceMode.value === 'trailer') {
      return t('player.trailerTitle', { title: detailTitle.value })
    }

    if (entryDetailsSurfaceMode.value === 'media') {
      return t('media.videoTitle', { title: detailTitle.value })
    }

    return null
  })

  /**
   * Exposes the poster URL applied to the unified Video.js renderer.
   *
   * @returns The media poster URL in entry details mode, or the standalone poster URL.
   */
  const activeVideoPoster = computed(() =>
    entryDetailsSurfaceMode.value === 'media' ? props.mediaPosterUrl : props.poster,
  )

  /**
   * Exposes the overlay logo URL applied to the unified Video.js renderer.
   *
   * @returns The media overlay logo URL for entry details surfaces, or null for standalone.
   */
  const activeVideoOverlayLogoUrl = computed(() =>
    (entryDetailsSurfaceMode.value === 'media' || entryDetailsSurfaceMode.value === 'trailer') ? props.mediaOverlayLogoUrl : null,
  )

  /**
   * Exposes the autoplay setting applied to the unified Video.js renderer.
   *
   * @returns True for trailer in entry details mode, media autoplay setting for media surface, or standalone autoplay prop.
   */
  const activeVideoAutoplay = computed(() =>
    entryDetailsSurfaceMode.value === 'media' ? props.mediaAutoplay :
      entryDetailsSurfaceMode.value === 'trailer' ? true : props.autoplay,
  )

  /**
   * Exposes the muted setting applied to the unified Video.js renderer.
   *
   * @returns False for entry details mode, or the standalone muted prop.
   */
  const activeVideoMuted = computed(() =>
    isEntryDetailsMode.value ? false : props.muted,
  )

  /**
   * Exposes the loop setting applied to the unified Video.js renderer.
   *
   * @returns False for entry details mode, or the standalone loop prop.
   */
  const activeVideoLoop = computed(() =>
    isEntryDetailsMode.value ? false : props.loop,
  )

  /**
   * Exposes the controls visibility setting applied to the unified Video.js renderer.
   *
   * @returns True for entry details mode, or the standalone controls prop.
   */
  const activeVideoControls = computed(() =>
    isEntryDetailsMode.value ? true : props.controls,
  )

  /**
   * Exposes the playsinline setting applied to the unified Video.js renderer.
   *
   * @returns True for entry details mode, or the standalone playsinline prop.
   */
  const activeVideoPlaysinline = computed(() =>
    isEntryDetailsMode.value ? true : props.playsinline,
  )

  /**
   * Exposes the preload setting applied to the unified Video.js renderer.
   *
   * @returns 'metadata' for entry details mode, or the standalone preload prop.
   */
  const activeVideoPreload = computed(() =>
    isEntryDetailsMode.value ? 'metadata' : props.preload,
  )

  /**
   * Exposes the aria hidden setting applied to the unified Video.js renderer.
   *
   * @returns False for entry details mode, or the standalone ariaHidden prop.
   */
  const activeVideoAriaHidden = computed(() =>
    isEntryDetailsMode.value ? false : props.ariaHidden,
  )

  /**
   * Exposes the tab index applied to the unified Video.js renderer.
   *
   * @returns Null for entry details mode, or the standalone tabIndex prop.
   */
  const activeVideoTabIndex = computed(() =>
    isEntryDetailsMode.value ? null : props.tabIndex,
  )

  /**
   * Exposes the initial playback time applied to the unified Video.js renderer.
   *
   * @returns The initial playback time for media surface or standalone, or null otherwise.
   */
  const activeVideoInitialPlaybackTime = computed(() =>
    !isEntryDetailsMode.value || entryDetailsSurfaceMode.value === 'media'
      ? props.initialPlaybackTime
      : null,
  )

  /**
   * Indicates whether the active source follows a language or player selection change.
   *
   * @returns True when its preceding playback position and state should be restored.
   */
  const activeVideoPreservePlaybackOnSourceSwitch = computed(() =>
    activeVideoSource.value?.src === sourcePreservingPlayback.value,
  )

  /**
   * Indicates whether the episode autoplay toggle should be shown.
   *
   * @returns True when the media surface is active and episode autoplay toggle is enabled.
   */
  const activeVideoShowEpisodeAutoplayToggle = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.showEpisodeAutoplayToggle,
  )

  /**
   * Indicates whether episode autoplay is currently enabled.
   *
   * @returns True when the media surface is active and episode autoplay is enabled.
   */
  const activeVideoIsEpisodeAutoplayEnabled = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.isEpisodeAutoplayEnabled,
  )

  /**
   * Indicates whether video navigation controls should be shown in the player control bar.
   *
   * @returns True when the media surface is active and navigation controls are enabled.
   */
  const activeVideoShowVideoNavigationControls = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.showVideoNavigationControls,
  )

  /**
   * Indicates whether there is a previous video available to navigate to.
   *
   * @returns True when there is a previous playable video.
   */
  const activeVideoHasPreviousVideo = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.hasPreviousVideo,
  )

  /**
   * Indicates whether there is a next video available to navigate to.
   *
   * @returns True when there is a next playable video.
   */
  const activeVideoHasNextVideo = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.hasNextVideo,
  )

  /**
   * Indicates whether the big play button should be displayed on the Video.js player.
   * Controls visibility based on the controls prop, with CSS handling loading state.
   *
   * @returns True when the big play button should be shown.
   */
  const activeVideoShowBigPlayButton = computed(() => activeVideoControls.value)

  /**
   * Exposes the classes applied to the unified Video.js renderer.
   *
   * @returns CSS class for the video element.
   */
  const activeVideoClass = computed(() =>
    isEntryDetailsMode.value
      ? 'entry-details__trailer-frame entry-details__trailer-frame--video'
      : 'video-player__video--standalone',
  )

  return {
    activeVideoSource,
    activeVideoAriaLabel,
    activeVideoPoster,
    activeVideoOverlayLogoUrl,
    activeVideoAutoplay,
    activeVideoMuted,
    activeVideoLoop,
    activeVideoControls,
    activeVideoPlaysinline,
    activeVideoPreload,
    activeVideoAriaHidden,
    activeVideoTabIndex,
    activeVideoInitialPlaybackTime,
    activeVideoPreservePlaybackOnSourceSwitch,
    activeVideoShowEpisodeAutoplayToggle,
    activeVideoIsEpisodeAutoplayEnabled,
    activeVideoShowVideoNavigationControls,
    activeVideoHasPreviousVideo,
    activeVideoHasNextVideo,
    activeVideoShowBigPlayButton,
    activeVideoClass,
  }
}
