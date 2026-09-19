import videojs from 'video.js'
import 'video.js/dist/video-js.css'
import '@videojs/themes/dist/city/index.css'
import 'videojs-contrib-quality-menu/dist/videojs-contrib-quality-menu.css'
import 'videojs-contrib-eme'
import 'videojs-contrib-quality-menu'
import 'videojs-hotkeys'
import 'videojs-sprite-thumbnails'
import { computed, onBeforeUnmount, shallowRef, watch } from 'vue'

import type { ResolvedVideoMediaSource } from '@/services/players'
import type { VideoJsPlayerState } from '@/types/media'

import { PLAYBACK_SAVE_STEP_SECONDS, REMAINING_TIME_CLASS } from '@/composables/video/video-js-media-renderer/constants'
import {
  installAdaptiveControlBarMenuPositioning,
  installSeekOnClick,
  installTimerToggle,
  syncEpisodeAutoplayToggleControl,
  syncPrevNextVideoControls,
} from '@/composables/video/video-js-media-renderer/controls'
import { installStableFullscreenBridge } from '@/composables/video/video-js-media-renderer/fullscreen'
import { installChapterOverlay, installChapterSegments, installSkipChapterButton } from '@/composables/video/video-js-media-renderer/chapters'
import {
  buildDashKeySystemOptions,
  installDashPeriodChapters,
  installDashQualityBridge,
  isDashSource,
  type DashQualityBridge,
} from '@/composables/video/video-js-media-renderer/dash'
import {
  isDurationAvailable,
  isLiveStream,
  syncDurationAvailabilityState,
  updateLiveProgressBar,
} from '@/composables/video/video-js-media-renderer/live'
import {
  disableControlBarMenuHoverBehavior,
  getSelectedQualityLabel,
  resolveQualityPreferenceToken,
  selectPreferredVhsPlaylist,
  syncDisplayedQualityPreference,
} from '@/composables/video/video-js-media-renderer/quality'
import {
applyPersistedPlayerState,
  capturePlayerState,
  getPlaybackEventSourceUrl,
  restorePersistedTrackPreferences,
  resolveRetainedQuality,
} from '@/composables/video/video-js-media-renderer/state'
import type {
  UseVideoJsMediaRendererOptions,
  VideoJsMediaRendererEmits,
  VideoJsMediaRendererProps,
  VideoJsPlayer,
  VideoJsSpriteThumbnailsPlugin,
  VideoJsSourceInput,
  VideoJsTechHandle,
} from '@/composables/video/video-js-media-renderer/types'
import { t } from '@/i18n'
import { markImageUrlFailed } from '@/composables/media/useFailedImageUrls'

const STORYBOARD_VTT_REQUEST_TIMEOUT_MS = 5_000
/** Display size of a storyboard preview frame. */
const STORYBOARD_PREVIEW_FRAME_SIZE = 15
/** Display width, in pixels, of a storyboard preview frame. */
const STORYBOARD_PREVIEW_WIDTH = 16 * STORYBOARD_PREVIEW_FRAME_SIZE
/** Display height, in pixels, of a storyboard preview frame. */
const STORYBOARD_PREVIEW_HEIGHT = 9 * STORYBOARD_PREVIEW_FRAME_SIZE
/** Player width below which the sprite plugin applies its responsive factor. */
const STORYBOARD_RESPONSIVE_WIDTH = 600
/** Gap kept between the storyboard tooltip and the seek bar. */
const STORYBOARD_TOOLTIP_GAP = 12
/** Interval between attempts to attach the dash.js quality bridge. */
const DASH_QUALITY_BRIDGE_RETRY_INTERVAL = 250
/** Bounded window after which pending dash.js bridge attach attempts stop. */
const DASH_QUALITY_BRIDGE_RETRY_TIMEOUT = 10_000

type StoryboardVttCue = {
  imageUrl: string
  start: number
  end: number
  x: number
  y: number
  width: number
  height: number
}

type ResolvedSpriteThumbnailOptions = {
  options: Record<string, unknown>
  cues: StoryboardVttCue[]
}

export type {
  VideoJsMediaDimensions,
  VideoJsMediaRendererEmits,
  VideoJsMediaRendererProps,
} from '@/composables/video/video-js-media-renderer/types'

export type { VideoJsSourceInput } from '@/composables/video/video-js-media-renderer/types'

/**
 * Manages the lifecycle and UI integration of one Video.js renderer instance.
 *
 * @param options Component props, emits, and template refs required to control the player.
 * @returns Reactive view state consumed by the renderer component.
 */
export function useVideoJsMediaRenderer(options: UseVideoJsMediaRendererOptions) {
  const { props, emit, hostElement, videoElement } = options
  /** Current active Video.js player instance. */
  const activePlayer = shallowRef<VideoJsPlayer | null>(null)
  /** Whether the poster overlay is currently visible. */
  const isPosterOverlayVisible = shallowRef(false)
  /** Whether the video is in initial loading state. */
  const isVideoInitialLoading = shallowRef(true)

   /** Cleanup function for pending source restore operation. */
   let pendingSourceRestoreCleanup: (() => void) | null = null
   /** Cleanup function for pending quality selector override. */
   let pendingQualitySelectorCleanup: (() => void) | null = null
   /** dash.js quality bridge attached to the active DASH source. */
   let activeDashQualityBridge: DashQualityBridge | null = null
   /** Cleanup function for the pending DASH quality bridge installation. */
   let pendingDashQualityBridgeCleanup: (() => void) | null = null
   /** Sprite thumbnail plugin instance attached to the active player. */
   let spriteThumbnailsPlugin: VideoJsSpriteThumbnailsPlugin | null = null
 /** Preferred quality selected by the user. */
     let preferredQuality: string | null = null
    /** Latest state used to restore tracks after DASH renews the Video.js track lists. */
    let retainedPlayerState: VideoJsPlayerState | null = null
   /** Whether the video initial load complete event has been emitted. */
   let hasEmittedInitialLoadComplete = false
    /** Whether the video metadata loaded event has been emitted. */
    let hasEmittedCurrentSourceMetadata = false
    /** Monotonic identifier used to discard stale asynchronous storyboard loads. */
    let storyboardLoadId = 0
    /** Exact VTT cue geometry used to override the plugin's uniform interval calculation. */
    let storyboardVttCues: StoryboardVttCue[] = []
    /** Resolved VTT storyboard grid info (cell size + grid dimensions). */
    const vttStoryboardGrid = shallowRef<{
      width: number; height: number; columns: number; rows: number
    } | null>(null)
    /** Fixed 16:9 storyboard preview size used for the tooltip layout box. */
    const storyboardPreviewSize = computed(() => {
      // Read the player width so the preview follows the plugin responsive factor
      // applied below 600 px wide players.
      const playerWidth = activePlayer.value?.currentWidth() ?? 0
      const grid = vttStoryboardGrid.value
      const storyboard = props.source.storyboard
      const cellWidth = grid?.width ?? storyboard?.width
      const cellHeight = grid?.height ?? storyboard?.height
      const columns = grid?.columns ?? storyboard?.columns ?? 0

      if (!cellWidth || !cellHeight) {
        return null
      }

      // Fit the cell preview into the fixed 16:9 frame with a uniform scale so the
      // source aspect ratio is preserved (an independent x/y scale would stretch
      // off-ratio cells and deform the thumbnail).
      const fitScale = Math.min(
        STORYBOARD_PREVIEW_WIDTH / cellWidth,
        STORYBOARD_PREVIEW_HEIGHT / cellHeight,
      )
      const responsiveScale = playerWidth > 0 && playerWidth < STORYBOARD_RESPONSIVE_WIDTH
        ? playerWidth / STORYBOARD_RESPONSIVE_WIDTH
        : 1
      const scale = fitScale * responsiveScale

      return {
        displayWidth: scale * cellWidth,
        displayHeight: scale * cellHeight,
        columns,
        fitScale,
        responsiveScale,
        displayToSourceScale: scale,
      }
    })

   /**
    * Emits the video initial load complete event once per player instance.
    */
   function emitVideoInitialLoadComplete() {
     if (!hasEmittedInitialLoadComplete) {
       hasEmittedInitialLoadComplete = true
       emit('video-initial-load-complete')
     }
   }

   /**
    * Emits the natural video dimensions once metadata is available.
    */
   function emitVideoMetadataLoaded() {
     if (hasEmittedCurrentSourceMetadata) {
       return
     }

     const element = videoElement.value
     const width = element?.videoWidth ?? 0
     const height = element?.videoHeight ?? 0

     if (width <= 0 || height <= 0) {
       return
     }

     emit('video-metadata-loaded', {
       width,
       height,
       aspectRatio: width / height,
     })
     hasEmittedCurrentSourceMetadata = true
   }

   /**
    * Indicates whether the logo overlay should stay mounted before playback starts.
    */
  const shouldRenderPosterOverlay = computed(() =>
    Boolean(props.overlayLogoUrl),
  )

  /**
   * Emits one sanitized playback position and duration to the parent controller.
   *
   * @param value Playback position in seconds, or `null` when the stored progress should be cleared.
   * @param duration Total media duration in seconds, or `null` when unavailable.
   */
  function emitPlaybackProgress(value: number | null, duration: number | null = null) {
    const sanitizedPlaybackTime =
      typeof value === 'number' && Number.isFinite(value) && value > 0
        ? value
        : null
    const sanitizedDuration =
      typeof duration === 'number' && Number.isFinite(duration) && duration > 0
        ? duration
        : null

    emit('update:playback-progress', sanitizedPlaybackTime, sanitizedDuration)
  }

  /**
   * Notifies the parent controller that playback has effectively started.
   *
   * @param sourceUrl Active source URL reported by the player.
   */
  function emitPlaybackStarted(sourceUrl: string | null) {
    emit('playback-started', sourceUrl)
  }

  /**
   * Notifies the parent controller that playback reached the end of the current source.
   */
  function emitPlaybackEnded() {
    emit('playback-ended')
  }

  /**
   * Notifies the parent controller that the autoplay preference changed from the player chrome.
   *
   * @param value Indicates whether the next episode should start automatically.
   */
  function emitEpisodeAutoplayEnabledUpdate(value: boolean) {
    emit('update:is-episode-autoplay-enabled', value)
  }

  /**
   * Emits the latest persisted Video.js UI state to the parent controller.
   *
   * @param value Captured player state, or `null` when unavailable.
   */
  function emitPlayerState(value: VideoJsPlayerState | null) {
    emit('update:player-state', value)
  }

  /**
   * Emits the latest player state when the current instance is available.
   *
   * @param player Video.js player currently bound to the renderer.
   */
  function emitCurrentPlayerState(player: VideoJsPlayer) {
    retainedPlayerState = capturePlayerState(player, preferredQuality)
    emitPlayerState(retainedPlayerState)
  }

  /**
   * Removes the transient source-switch listeners attached for one in-flight media transition.
   */
  function clearPendingSourceRestoreCleanup() {
    pendingSourceRestoreCleanup?.()
    pendingSourceRestoreCleanup = null
  }

  /**
   * Removes the temporary VHS selector override attached for the active source.
   */
  function clearPendingQualitySelectorCleanup() {
    pendingQualitySelectorCleanup?.()
    pendingQualitySelectorCleanup = null
  }

  /**
   * Releases the dash.js quality bridge attached to the active source.
   */
  function clearPendingDashQualityBridge() {
    pendingDashQualityBridgeCleanup?.()
    pendingDashQualityBridgeCleanup = null
    activeDashQualityBridge = null
  }

   /**
    * Disposes the current Video.js instance before the renderer switches source or unmounts.
    */
   function destroyActivePlayer() {
     clearPendingSourceRestoreCleanup()
     clearPendingQualitySelectorCleanup()
     clearPendingDashQualityBridge()

     if (activePlayer.value) {
       emitCurrentPlayerState(activePlayer.value)
     }

     activePlayer.value?.dispose()
     activePlayer.value = null
     spriteThumbnailsPlugin = null
     retainedPlayerState = null
     isPosterOverlayVisible.value = false
     hasEmittedInitialLoadComplete = false
   }

  /**
   * Applies the reactive accessibility and autoplay attributes directly to the video element.
   *
   * @param element Native video element managed by Video.js.
   */
  function syncVideoElementAttributes(element: HTMLVideoElement) {
    element.style.setProperty('--vjs-theme-city--primary', '#46a3ff')
    element.style.setProperty('--vjs-theme-city--secondary', '#f7fbff')
    element.defaultMuted = props.muted ?? false
    element.muted = props.muted ?? false
    element.autoplay = props.autoplay ?? false
    element.loop = props.loop ?? false
    element.playsInline = props.playsinline ?? false
    element.preload = props.preload ?? 'metadata'

    if (props.poster) {
      element.poster = props.poster
    } else {
      element.removeAttribute('poster')
    }

    if (props.ariaHidden) {
      element.setAttribute('aria-hidden', 'true')
    } else {
      element.removeAttribute('aria-hidden')
    }

    if (props.videoAriaLabel) {
      element.setAttribute('aria-label', props.videoAriaLabel)
    } else {
      element.removeAttribute('aria-label')
    }

    if (props.tabIndex === null || props.tabIndex === undefined) {
      element.removeAttribute('tabindex')
    } else {
      element.tabIndex = props.tabIndex
    }
  }

  /**
   * Hides the Video.js poster image when its source fails to load and advances
   * any poster fallback chain.
   *
   * Video.js renders the poster as an `<img>` inside `.vjs-poster` and does not
   * hide it automatically when the image cannot be fetched. The failed URL is also
   * recorded so page-level poster resolvers skip it like a `null` candidate and
   * fall back to the next available image.
   *
   * @param player Video.js player currently bound to the renderer.
   */
  function installPosterImageErrorHandler(player: VideoJsPlayer) {
    const install = () => {
      const playerElement = player.el()
      const posterImage = playerElement?.querySelector<HTMLImageElement>('.vjs-poster img')

      if (!posterImage) {
        return
      }

      if (posterImage.dataset.posterErrorHandlerInstalled === 'true') {
        return
      }
      posterImage.dataset.posterErrorHandlerInstalled = 'true'

      posterImage.addEventListener('error', () => {
        markImageUrlFailed(props.poster)
        const posterElement = player.el()?.querySelector<HTMLElement>('.vjs-poster')
        posterElement?.setAttribute('hidden', 'true')
        player.poster('')
      })
    }

    // Video.js (re)creates the `<img>` whenever the poster source changes, so the
    // handler must be (re)attached when the poster-change event surfaces the element.
    install()
    player.on('posterchange', install)
  }

  /**
   * Preloads the first sprite image and derives one cell's dimensions from its declared grid.
   *
   * @param url Sprite image URL, which may contain an `{index}` placeholder.
   * @param firstPageIndex First index used by a sequential sprite URL.
   * @param columns Number of sprite columns.
   * @param rows Number of sprite rows.
   * @returns Derived cell dimensions and a normalized sprite URL, or `null` when the grid is invalid.
   */
  function resolveSpriteThumbnailDimensions(
    url: string,
    firstPageIndex: number,
    columns: number,
    rows: number,
  ): Promise<{ url: string, width: number, height: number } | null> {
    const firstSpriteUrl = url.replace('{index}', String(firstPageIndex))

    return new Promise((resolve) => {
      const image = new Image()
      image.onload = () => {
        const horizontalGap = findSpriteGridGap(image.naturalWidth, columns)
        const verticalGap = findSpriteGridGap(image.naturalHeight, rows)

        if (horizontalGap === null || verticalGap === null) {
          console.warn('[Storyboard] Sprite image dimensions are incompatible with its declared grid.', {
            url: firstSpriteUrl,
            naturalWidth: image.naturalWidth,
            naturalHeight: image.naturalHeight,
            columns,
            rows,
          })
          resolve(null)
          return
        }

        const width = (image.naturalWidth - horizontalGap * (columns - 1)) / columns
        const height = (image.naturalHeight - verticalGap * (rows - 1)) / rows

        if (horizontalGap === 0 && verticalGap === 0) {
          resolve({ url, width, height })
          return
        }

        if (url.includes('{index}')) {
          console.warn('[Storyboard] Cannot normalize grid separators in sequential sprite images.', {
            url: firstSpriteUrl,
          })
          resolve(null)
          return
        }

        try {
          const canvas = document.createElement('canvas')
          canvas.width = width * columns
          canvas.height = height * rows
          const context = canvas.getContext('2d')
          if (!context) {
            resolve(null)
            return
          }

          for (let row = 0; row < rows; row += 1) {
            for (let column = 0; column < columns; column += 1) {
              context.drawImage(
                image,
                column * (width + horizontalGap),
                row * (height + verticalGap),
                width,
                height,
                column * width,
                row * height,
                width,
                height,
              )
            }
          }

          resolve({ url: canvas.toDataURL('image/jpeg', 0.92), width, height })
        } catch (error) {
          console.warn('[Storyboard] Unable to normalize sprite image grid separators.', {
            url: firstSpriteUrl,
            error,
          })
          resolve(null)
        }
      }
      image.onerror = () => {
        console.warn('[Storyboard] Unable to preload sprite image for dimension inference.', {
          url: firstSpriteUrl,
        })
        resolve(null)
      }
      image.src = firstSpriteUrl
    })
  }

  /**
   * Finds the smallest uniform gap between cells in one sprite axis.
   *
   * @param size Natural image width or height.
   * @param cells Number of cells along that axis.
   * @returns Gap size in pixels, or `null` when no regular grid fits the image.
   */
  function findSpriteGridGap(size: number, cells: number): number | null {
    for (let gap = 0; gap <= 32; gap += 1) {
      const cellSize = size - gap * (cells - 1)
      if (cellSize > 0 && cellSize % cells === 0) {
        return gap
      }
    }

    return null
  }

  /**
   * Builds sprite thumbnail plugin options from storyboard metadata.
   *
   * Missing cell dimensions are inferred by preloading the first sprite image.
   *
   * @param source Resolved source selected for playback.
   * @returns Sprite thumbnail plugin configuration.
   */
  async function buildSpriteThumbnailOptions(source: ResolvedVideoMediaSource): Promise<Record<string, unknown>> {
    if (!source.storyboard) {
      return {}
    }

    const { firstPageIndex, interval, width, height, ...spriteThumbnails } = source.storyboard
    const resolvedSprite = width && height
      ? { url: source.storyboard.url, width, height }
      : await resolveSpriteThumbnailDimensions(
        source.storyboard.url,
        firstPageIndex,
        source.storyboard.columns,
        source.storyboard.rows,
      )

    if (!resolvedSprite) {
      return {}
    }

    const resolvedSpriteThumbnails = { ...spriteThumbnails, ...resolvedSprite }
    const options = interval === null
      ? resolvedSpriteThumbnails
      : { ...resolvedSpriteThumbnails, interval }

    return firstPageIndex > 0
      ? { ...options, idxTag: (index: number) => index + firstPageIndex }
      : options
  }

  /**
   * Converts a timestamp from a WebVTT cue into seconds.
   *
   * @param value WebVTT timestamp in `HH:MM:SS.mmm` form.
   * @returns Timestamp in seconds, or null when invalid.
   */
  function parseStoryboardVttTimestamp(value: string): number | null {
    const match = /^(?:(\d+):)?(\d{2}):(\d{2}(?:\.\d+)?)$/.exec(value.trim())
    if (!match) {
      return null
    }

    const hours = Number(match[1] ?? 0)
    const minutes = Number(match[2])
    const seconds = Number(match[3])
    const total = hours * 3_600 + minutes * 60 + seconds

    return Number.isFinite(total) && total >= 0 ? total : null
  }

  /**
   * Parses image cue geometry from a storyboard WebVTT document.
   *
   * @param documentText Raw WebVTT document.
   * @param vttUrl Absolute URL of the WebVTT document.
   * @returns Parsed cue metadata for one sprite image, or an empty list.
   */
  function parseStoryboardVttCues(documentText: string, vttUrl: string): StoryboardVttCue[] {
    const cuePattern = /(?:^|\n)\s*((?:\d+:)?\d{2}:\d{2}(?:\.\d+)?)\s+-->\s+((?:\d+:)?\d{2}:\d{2}(?:\.\d+)?)[^\n]*\n\s*([^\s#]+)#xywh=(\d+),(\d+),(\d+),(\d+)/g
    const cues: StoryboardVttCue[] = []

    for (const match of documentText.matchAll(cuePattern)) {
      const [, startText, endText, imageUrl, xText, yText, widthText, heightText] = match
      if (!startText || !endText || !imageUrl || !xText || !yText || !widthText || !heightText) {
        continue
      }

      const start = parseStoryboardVttTimestamp(startText)
      const end = parseStoryboardVttTimestamp(endText)
      const x = Number(xText)
      const y = Number(yText)
      const width = Number(widthText)
      const height = Number(heightText)

      if (
        start === null ||
        end === null ||
        end <= start ||
        !Number.isInteger(x) ||
        !Number.isInteger(y) ||
        !Number.isInteger(width) ||
        !Number.isInteger(height) ||
        width <= 0 ||
        height <= 0
      ) {
        continue
      }

      let resolvedImageUrl = imageUrl
      try {
        resolvedImageUrl = new URL(imageUrl, vttUrl).href
      } catch {
        continue
      }

      cues.push({
        imageUrl: resolvedImageUrl,
        start,
        end,
        x,
        y,
        width,
        height,
      })
    }

    return cues
  }

  /**
   * Derives sprite thumbnail plugin options from a storyboard WebVTT document.
   *
   * @param documentText Raw WebVTT document.
   * @param vttUrl Absolute URL of the WebVTT document.
   * @returns Plugin configuration, or null for unsupported cue layouts.
   */
  function parseStoryboardVttOptions(
    documentText: string,
    vttUrl: string,
  ): ResolvedSpriteThumbnailOptions | null {
    const cues = parseStoryboardVttCues(documentText, vttUrl)
    const firstCue = cues[0]

    if (!firstCue || cues.some((cue) =>
      cue.width !== firstCue.width ||
      cue.height !== firstCue.height,
    )) {
      if (!firstCue)
        return null;
      
      cues.forEach(entry => {
        entry.width = firstCue.width;
        entry.height = firstCue.height;
      })
      //return null
    }

    const urlArray = [...new Set(cues.map((cue) => cue.imageUrl))]
    const columns = Math.max(...cues.map((cue) => cue.x / cue.width + 1))
    const rows = Math.max(...cues.map((cue) => cue.y / cue.height + 1))
    const lastCue = cues[cues.length - 1]
    const interval = lastCue
      ? lastCue.end / cues.length
      : cues[1]
        ? cues[1].start - firstCue.start
        : firstCue.end - firstCue.start

    if (
      !Number.isInteger(columns) ||
      !Number.isInteger(rows) ||
      columns < 1 ||
      rows < 1 ||
      !Number.isFinite(interval) ||
      interval <= 0
    ) {
      return null
    }

    return {
      options: {
        urlArray,
        width: firstCue.width,
        height: firstCue.height,
        columns,
        rows,
        interval,
        downlink: 0,
      },
      cues,
    }
  }

  /**
   * Resolves VTT storyboard metadata to the sprite plugin configuration, retaining the declared
   * sprite storyboard as a fallback when the VTT cannot be fetched or parsed.
   *
   * @param source Resolved source selected for playback.
   * @returns Sprite thumbnail plugin configuration.
   */
  async function resolveSpriteThumbnailOptions(
    source: ResolvedVideoMediaSource,
  ): Promise<ResolvedSpriteThumbnailOptions> {
    if (!source.storyboardVttUrl) {
      return { options: await buildSpriteThumbnailOptions(source), cues: [] }
    }

    const controller = new AbortController()
    const timeout = window.setTimeout(() => controller.abort(), STORYBOARD_VTT_REQUEST_TIMEOUT_MS)

    try {
      const response = await fetch(source.storyboardVttUrl, { signal: controller.signal })
      if (!response.ok) {
        return { options: await buildSpriteThumbnailOptions(source), cues: [] }
      }

      return parseStoryboardVttOptions(await response.text(), source.storyboardVttUrl)
        ?? { options: await buildSpriteThumbnailOptions(source), cues: [] }
    } catch {
      return { options: await buildSpriteThumbnailOptions(source), cues: [] }
    } finally {
      window.clearTimeout(timeout)
    }
  }

  /**
   * Applies exact VTT cue geometry and the fixed preview size after the sprite
   * plugin updates its uniform-grid preview.
   *
   * The plugin only supports one fixed interval, whereas valid VTT documents can contain cues
   * with different durations. Deferring the override keeps plugin layout behavior while selecting
   * the image region declared for the actual hover timestamp.
   *
   * The tooltip layout box is resized here instead of CSS-scaled so Video.js measures
   * the displayed size when clamping the tooltip inside the player near the seek bar edges.
   *
   * @param player Video.js player owning the seek bar and thumbnail tooltip.
   */
  function installStoryboardVttCueOverride(player: VideoJsPlayer) {
    const playerElement = player.el()
    const seekBarElement = playerElement?.querySelector<HTMLElement>('.vjs-progress-control')

    if (!playerElement || !seekBarElement) {
      return
    }

    const rescaleStoryboardTooltip = () => {
      const previewSize = storyboardPreviewSize.value
      const tooltip = playerElement.querySelector<HTMLElement>('.vjs-mouse-display .vjs-time-tooltip')

      if (!previewSize || !tooltip) {
        return
      }

      const pluginWidth = Number.parseFloat(tooltip.style.width)
      const pluginHeight = Number.parseFloat(tooltip.style.height)

      // The sprite plugin rewrites these inline styles on every mousemove, so skip
      // tooltips without plugin geometry.
      if (
        !Number.isFinite(pluginWidth) || pluginWidth <= 0 ||
        !Number.isFinite(pluginHeight) || pluginHeight <= 0
      ) {
        return
      }

      tooltip.style.width = `${previewSize.displayWidth}px`
      tooltip.style.height = `${previewSize.displayHeight}px`
      tooltip.style.top = `${-(previewSize.displayHeight + STORYBOARD_TOOLTIP_GAP)}px`
      tooltip.style.backgroundPosition = tooltip.style.backgroundPosition
        .split(' ')
        .map((component) => {
          const value = Number.parseFloat(component)

          return Number.isFinite(value) ? `${value * previewSize.fitScale}px` : component
        })
        .join(' ')

      if (previewSize.columns > 0) {
        const backgroundSize = tooltip.style.backgroundSize.split(' ')
        const backgroundHeight = backgroundSize.length > 1
          ? backgroundSize.slice(1).join(' ').trim() || 'auto'
          : 'auto'

        tooltip.style.backgroundSize = `${pluginWidth * previewSize.fitScale * previewSize.columns}px ${backgroundHeight}`
      }
    }

    const handleMouseMove = (event: MouseEvent) => {
      queueMicrotask(rescaleStoryboardTooltip)

      const duration = player.duration()
      const bounds = seekBarElement.getBoundingClientRect()

      if (
        storyboardVttCues.length === 0 ||
        typeof duration !== 'number' || !Number.isFinite(duration) || duration <= 0 ||
        bounds.width <= 0
      ) {
        return
      }

      const progress = Math.min(1, Math.max(0, (event.clientX - bounds.left) / bounds.width))
      const time = progress * duration
      const cue = storyboardVttCues.find(({ start, end }) => time >= start && time < end)
        ?? (time === duration ? storyboardVttCues[storyboardVttCues.length - 1] : undefined)

      if (!cue) {
        return
      }

      queueMicrotask(() => {
        const tooltip = playerElement.querySelector<HTMLElement>('.vjs-mouse-display .vjs-time-tooltip')
        if (!tooltip) {
          return
        }

        tooltip.style.backgroundImage = `url("${cue.imageUrl}")`
        tooltip.style.backgroundPosition = `${-cue.x}px ${-cue.y}px`
        rescaleStoryboardTooltip()
      })
    }

    seekBarElement.addEventListener('mousemove', handleMouseMove)
    player.on('dispose', () => {
      seekBarElement.removeEventListener('mousemove', handleMouseMove)
    })
  }

  /**
   * Sets the storyboard interval derived from the loaded video duration when the backend omits it.
   *
   * @param player Video.js player with available media metadata.
   * @param source Source whose storyboard may require an inferred interval.
   */
  function syncDerivedStoryboardInterval(
    player: VideoJsPlayer,
    source: ResolvedVideoMediaSource,
  ) {
    const storyboard = source.storyboard

    if (
      source.storyboardVttUrl ||
      !storyboard ||
      storyboard.interval !== null ||
      !spriteThumbnailsPlugin
    ) {
      return
    }

    const duration = player.duration()
    const thumbnailCount = storyboard.rows * storyboard.columns
    const interval = typeof duration === 'number' && Number.isFinite(duration) && duration > 0
      ? duration / thumbnailCount
      : null

    if (interval === null || !Number.isFinite(interval) || interval <= 0) {
      return
    }

    spriteThumbnailsPlugin.options.interval = interval
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

   /**
    * Updates the control bar with the current autoplay toggle state.
    *
    * @param player Video.js player currently bound to the renderer.
    */
   function syncEpisodeAutoplayControl(player: VideoJsPlayer) {
     syncEpisodeAutoplayToggleControl(player, {
       controls: props.controls ?? false,
       showEpisodeAutoplayToggle: props.showEpisodeAutoplayToggle ?? false,
       isEpisodeAutoplayEnabled: props.isEpisodeAutoplayEnabled ?? false,
       onEpisodeAutoplayToggle: () => {
         emitEpisodeAutoplayEnabledUpdate(!(props.isEpisodeAutoplayEnabled ?? false))
       },
     })
   }

   /**
    * Updates the control bar with prev/next video navigation controls.
    *
    * @param player Video.js player currently bound to the renderer.
    */
   function syncPrevNextVideoControl(player: VideoJsPlayer) {
     syncPrevNextVideoControls(player, {
       controls: props.controls ?? false,
       showPrevVideoControl: props.showVideoNavigationControls ?? false,
       showNextVideoControl: props.showVideoNavigationControls ?? false,
        hasPreviousVideo: props.hasPreviousVideo ?? false,
        hasNextVideo: props.hasNextVideo ?? false,
        previousVideoTitle: props.previousVideoTitle ?? null,
        nextVideoTitle: props.nextVideoTitle ?? null,
       onPrevVideo: () => {
         emit('navigate-video', -1)
       },
       onNextVideo: () => {
         emit('navigate-video', 1)
       },
     })
   }

   /**
    * Applies the host attribute used to suppress the big play button without waiting for a Vue render.
    *
    * @param shouldSuppressBigPlayButton Indicates whether the big play overlay should stay hidden.
    */
   function syncBigPlaySuppressionAttribute(shouldSuppressBigPlayButton: boolean) {
     const element = hostElement.value

     if (!element) {
       return
     }

     if (shouldSuppressBigPlayButton) {
       element.setAttribute('data-vjs-initial-loading', 'true')
       return
     }

     element.removeAttribute('data-vjs-initial-loading')
   }

   /**
    * Marks the video as being in its initial loading phase.
    */
   function markVideoInitialLoadStart() {
     isVideoInitialLoading.value = true
     hasEmittedCurrentSourceMetadata = false
     syncBigPlaySuppressionAttribute(true)
   }

   /**
    * Marks the video as having finished initial loading.
    */
   function markVideoInitialLoadComplete() {
     isVideoInitialLoading.value = false
     syncBigPlaySuppressionAttribute(props.isExternalLoading ?? false)
   }

  /**
   * Applies the reactive configuration that can change without recreating the player.
   *
   * @param player Video.js player currently bound to the renderer.
   */
  function syncExistingPlayerConfiguration(player: VideoJsPlayer) {
    const element = videoElement.value

    if (element) {
      syncVideoElementAttributes(element)
    }

    player.poster(props.poster ?? '')
    installPosterImageErrorHandler(player)
    player.controls(props.controls ?? false)
    player.loop(props.loop ?? false)
    syncEpisodeAutoplayControl(player)
    syncPrevNextVideoControl(player)
  }

  /**
   * Starts playback when autoplay is enabled and the current source is ready enough.
   *
   * @param player Video.js player currently bound to the renderer.
   * @param autoplayOverride Indicates that the preceding source was playing.
   */
  function attemptAutoplay(player: VideoJsPlayer, autoplayOverride?: boolean) {
    if (!props.autoplay && !autoplayOverride) {
      return
    }

    const playPromise = player.play()

    if (!playPromise) {
      return
    }

    void playPromise.catch(() => {})
  }

  /**
   * Pins the VHS playlist selector to the preferred quality bucket for the current source.
   *
   * @param player Video.js player currently bound to the renderer.
   * @param quality Preferred quality captured from the previous video state.
   * @param expectedSourceUrl Source URL that should receive the selector override.
   */
  function configureQualityPreferenceSelector(
    player: VideoJsPlayer,
    quality: string | null,
    expectedSourceUrl: string,
  ) {
    clearPendingQualitySelectorCleanup()

    const qualityPreferenceToken = resolveQualityPreferenceToken(quality)

    if (!qualityPreferenceToken) {
      return
    }

    const applySelectorOverride = () => {
      if (activePlayer.value !== player || props.source.src !== expectedSourceUrl) {
        return false
      }

      const tech = player.tech(true) as VideoJsTechHandle | null
      const vhs = tech?.vhs

      if (!vhs) {
        return false
      }

      const fallbackSelectPlaylist = vhs.selectPlaylist.bind(vhs)
      vhs.selectPlaylist = function selectPreferredPlaylistWrapper() {
        return selectPreferredVhsPlaylist(
          this as typeof vhs,
          qualityPreferenceToken,
        ) ?? fallbackSelectPlaylist()
      }

      pendingQualitySelectorCleanup = () => {
        const currentTech = player.tech(true) as VideoJsTechHandle | null

        if (currentTech?.vhs === vhs) {
          vhs.selectPlaylist = fallbackSelectPlaylist
        }

        pendingQualitySelectorCleanup = null
      }

      return true
    }

    if (applySelectorOverride()) {
      return
    }

    const retryTimer = window.setTimeout(() => {
      applySelectorOverride()
    }, 0)

    pendingQualitySelectorCleanup = () => {
      window.clearTimeout(retryTimer)
      pendingQualitySelectorCleanup = null
    }
  }

  /**
   * Attaches the dash.js quality bridge to the DASH source just assigned to the player.
   *
   * The source handler creates its dash.js MediaPlayer asynchronously after
   * `player.src()` is applied, so the installation is retried on a short
   * interval until the engine is ready or the bounded window expires. Each
   * source application clears pending attempts, so stale retries never attach
   * a bridge to a replaced source.
   *
   * @param player Video.js player currently bound to the renderer.
   * @param quality Preferred quality captured from the previous video state.
   */
  function installDashQualityBridgeForSource(player: VideoJsPlayer, quality: string | null) {
    clearPendingDashQualityBridge()

    let elapsed = 0

    const retryTimer = window.setInterval(() => {
      elapsed += DASH_QUALITY_BRIDGE_RETRY_INTERVAL

      if (elapsed > DASH_QUALITY_BRIDGE_RETRY_TIMEOUT) {
        clearPendingDashQualityBridge()
        return
      }

      if (activePlayer.value !== player) {
        clearPendingDashQualityBridge()
        return
      }

      const bridge = installDashQualityBridge({
        player,
        preferredQuality: quality,
        restoreTrackPreferences: () => {
          if (activePlayer.value === player) {
            restorePersistedTrackPreferences(player, retainedPlayerState)
          }
        },
      })

      if (!bridge) {
        return
      }

      window.clearInterval(retryTimer)
      activeDashQualityBridge = bridge
      pendingDashQualityBridgeCleanup = () => {
        bridge.dispose()
        activeDashQualityBridge = null
        pendingDashQualityBridgeCleanup = null
      }
    }, DASH_QUALITY_BRIDGE_RETRY_INTERVAL)

    pendingDashQualityBridgeCleanup = () => {
      window.clearInterval(retryTimer)
      activeDashQualityBridge = null
      pendingDashQualityBridgeCleanup = null
    }
  }

  /**
   * Restores one persisted player state after a source switch or component remount.
   *
   * @param player Video.js player currently bound to the renderer.
   * @param state Persisted player state that should be restored.
   */
  function restorePlayerState(player: VideoJsPlayer, state: VideoJsPlayerState | null) {
    applyPersistedPlayerState(player, state)
  }

   /**
    * Applies one new media source to the current player while preserving the user state.
    *
    * @param player Video.js player currently bound to the renderer.
    * @param source Resolved source selected for playback.
    * @param playerState State snapshot that should survive the source switch.
    * @param switchTime Playback position captured before the source switch.
    * @param switchAutoplay Indicates whether the preceding source was playing.
    */
    async function applySourceToPlayer(
      player: VideoJsPlayer,
      source: ResolvedVideoMediaSource,
      playerState: VideoJsPlayerState | null,
      switchTime?: number | null,
      switchAutoplay?: boolean,
    ) {
      const loadId = ++storyboardLoadId
      clearPendingSourceRestoreCleanup()
      clearPendingQualitySelectorCleanup()
      clearPendingDashQualityBridge()
      markVideoInitialLoadStart()
      isPosterOverlayVisible.value = shouldRenderPosterOverlay.value && !(props.autoplay || switchAutoplay)
      preferredQuality = resolveRetainedQuality(preferredQuality, playerState)
      retainedPlayerState = playerState
      let retainedQuality = preferredQuality
      vttStoryboardGrid.value = null
      storyboardVttCues = []

      const resolvedStoryboard = await resolveSpriteThumbnailOptions(source)
      const { options: spriteThumbnailOptions, cues } = resolvedStoryboard
      if (loadId !== storyboardLoadId || activePlayer.value !== player) {
        return
      }
      storyboardVttCues = cues

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

      restorePlayerState(player, playerState)
      playerStateRestored = true
    }

    const syncRetainedQualityPreference = (treatMissingMenuAsUnavailable = false) => {
      // DASH periods expose their own representation set, so a missing menu entry must
      // not discard a preference that can apply again on a later period.
      const isDash = isDashSource(source)
      const isQualityPreferenceAvailable = syncDisplayedQualityPreference(
        player,
        retainedQuality,
        { treatMissingMenuAsUnavailable: isDash ? false : treatMissingMenuAsUnavailable },
      )

      if (!isQualityPreferenceAvailable) {
        retainedQuality = null
        preferredQuality = null
        clearPendingQualitySelectorCleanup()
        emitCurrentPlayerState(player)
        return
      }

      if (isDash) {
        activeDashQualityBridge?.applyQualityPreference(retainedQuality)
      }
    }

     const handleLoadedMetadata = () => {
       emitVideoMetadataLoaded()
       syncDerivedStoryboardInterval(player, source)
       disableControlBarMenuHoverBehavior(player)
       installSeekOnClick(player, {
         controls: props.controls ?? false,
       })
       restorePlaybackTime()
       restorePlayerUiState()
       syncRetainedQualityPreference()

        if (player.readyState() >= 3) {
          markVideoInitialLoadComplete()
          emitVideoInitialLoadComplete()
          attemptAutoplay(player, switchAutoplay)
        }
     }

      const handleCanPlay = () => {
        emitVideoMetadataLoaded()
        syncDerivedStoryboardInterval(player, source)
        restorePlaybackTime()
        restorePlayerUiState()
        syncRetainedQualityPreference(true)
        markVideoInitialLoadComplete()
        emitVideoInitialLoadComplete()
        attemptAutoplay(player, switchAutoplay)
      }

     const handlePlaying = () => {
       markVideoInitialLoadComplete()
       emitVideoInitialLoadComplete()
       clearPendingSourceRestoreCleanup()
     }

    syncExistingPlayerConfiguration(player)
    player.on('loadedmetadata', handleLoadedMetadata)
    player.on('canplay', handleCanPlay)
    player.on('playing', handlePlaying)
    pendingSourceRestoreCleanup = () => {
      player.off('loadedmetadata', handleLoadedMetadata)
      player.off('canplay', handleCanPlay)
      player.off('playing', handlePlaying)
    }
    player.src(buildPlayerSource(source, spriteThumbnailOptions))

    if (isDashSource(source)) {
      installDashQualityBridgeForSource(player, retainedQuality)
    } else {
      configureQualityPreferenceSelector(player, retainedQuality, source.src)
    }
  }

  /**
   * Creates one fresh Video.js player instance for the current element and source.
   *
   * @param element Native video element enhanced by Video.js.
   * @param source Resolved source selected for playback.
   */
   function createPlayer(element: HTMLVideoElement, source: ResolvedVideoMediaSource) {
     syncVideoElementAttributes(element)
     isPosterOverlayVisible.value = shouldRenderPosterOverlay.value && !props.autoplay
     markVideoInitialLoadStart()

     const player = videojs(element, {
       autoplay: props.autoplay ?? false,
       controls: props.controls ?? false,
       loop: props.loop ?? false,
       muted: props.muted ?? false,
       preload: props.preload ?? 'metadata',
       responsive: true,
       fluid: false,
       bigPlayButton: props.showBigPlayButton ?? (props.controls ?? false),
       controlBar: props.controls ?? false,
       inactivityTimeout: props.controls ? 2000 : 0,
       enableDocumentPictureInPicture: props.controls ?? false,
       liveui: true,
       // Required by the DASH source handler so dash.js owns text track rendering.
       html5: {
         nativeCaptions: false,
       },
       notSupportedMessage: t('errors.videoNotSupported'),
     }) as VideoJsPlayer

     activePlayer.value = player
     installStableFullscreenBridge(player, () => hostElement.value)

     player.on('error', () => {
       console.error('[Video.js] Playback error:', player.error())
       emit('source-error')
       markVideoInitialLoadComplete()
       emitVideoInitialLoadComplete()
     })

     player.ready(() => {
       const playerElement = player.el()

       // Initialize plugins after player is fully ready
       if (typeof player.eme === 'function') {
         player.eme()
       }

       if (typeof player.qualityLevels === 'function') {
         player.qualityLevels()
       }

       if (props.controls && typeof player.qualityMenu === 'function') {
         player.qualityMenu({
           defaultResolution: 'none',
           useResolutionLabels: true,
         })
       }

       if (props.controls && typeof player.hotkeys === 'function') {
         player.hotkeys({
           enableModifiersForNumbers: false,
           enableVolumeScroll: false,
           seekStep: 5,
           volumeStep: 0.1,
         })
       }

         if (props.controls && typeof player.spriteThumbnails === 'function') {
            spriteThumbnailsPlugin = player.spriteThumbnails({}) ?? null
         }
         installStoryboardVttCueOverride(player)

        if (props.controls) {
          const baseChapters = source.chapters ?? []
          const dashChaptersHandled = isDashSource(source) && installDashPeriodChapters(player, baseChapters)

          if (!dashChaptersHandled && baseChapters.length > 0) {
            installChapterOverlay(player, baseChapters)
            installChapterSegments(player, baseChapters)
            installSkipChapterButton(player, baseChapters, 'ads')
          }

          if (baseChapters.length > 0) {
            installSkipChapterButton(player, baseChapters, 'intro')
            installSkipChapterButton(player, baseChapters, 'outro')
          }
        }

       /** Last playback step that was saved to persist progress. */
       let lastSavedPlaybackStep = -1

       /**
        * Handles click events on the quality menu.
        * @param event - DOM click event.
        */
       const handleQualityMenuClick = (event: Event) => {
        const target = event.target as HTMLElement | null

        if (!target?.closest('.vjs-quality-menu-wrapper .vjs-menu-item')) {
          return
        }

        window.setTimeout(() => {
          preferredQuality = getSelectedQualityLabel(player)
          emitCurrentPlayerState(player)
        }, 0)
      }

      playerElement?.addEventListener('click', handleQualityMenuClick)

      /**
       * Emits the current player state after UI updates have settled.
       */
      const emitPlayerStateAfterUiUpdate = () => {
        window.setTimeout(() => {
          emitCurrentPlayerState(player)
        }, 0)
      }

      /**
       * Handles change events on text track settings.
       * @param event - DOM change event.
       */
      const handleTextTrackSettingsChange = (event: Event) => {
        const target = event.target as HTMLElement | null

        if (!target?.closest('.vjs-text-track-settings')) {
          return
        }

        emitPlayerStateAfterUiUpdate()
      }

      /**
       * Handles click events on text track settings button.
       * @param event - DOM click event.
       */
      const handleTextTrackSettingsClick = (event: Event) => {
        const target = event.target as HTMLElement | null

        if (!target?.closest('.vjs-text-track-settings .vjs-default-button')) {
          return
        }

        emitPlayerStateAfterUiUpdate()
      }

      const audioTracks = player.audioTracks?.()
      const textTracks = player.textTracks?.()

      audioTracks?.addEventListener?.('change', emitPlayerStateAfterUiUpdate)
      textTracks?.addEventListener?.('change', emitPlayerStateAfterUiUpdate)
      playerElement?.addEventListener('change', handleTextTrackSettingsChange, true)
      playerElement?.addEventListener('click', handleTextTrackSettingsClick, true)

      installTimerToggle(player, {
        controls: props.controls ?? false,
        onStateChange: () => {
          emitCurrentPlayerState(player)
        },
      })
      syncDurationAvailabilityState(player)
      syncEpisodeAutoplayControl(player)
      disableControlBarMenuHoverBehavior(player)
      installAdaptiveControlBarMenuPositioning(player)

      /**
       * Handles duration change events on the player.
       */
      const handleDurationChange = () => {
        syncDurationAvailabilityState(player)

        if (!isDurationAvailable(player)) {
          player.el()?.classList.remove(REMAINING_TIME_CLASS)
        }

        if (!isLiveStream(player) || !player.liveTracker) {
          return
        }

        player.liveTracker.seekable = () => {
          const seekable = player.seekable()
          return seekable && seekable.length > 0 ? seekable : null
        }

        player.liveTracker.liveCurrentTime = () => {
          return player.currentTime() || 0
        }

        updateLiveProgressBar(player)
      }

      player.on('durationchange', handleDurationChange)
      handleDurationChange()

      player.on('timeupdate', () => {
        const currentTime = player.currentTime()

        if (typeof currentTime !== 'number' || !Number.isFinite(currentTime) || currentTime <= 0) {
          return
        }

        if (isLiveStream(player)) {
          updateLiveProgressBar(player)
        }

        const playbackStep = Math.floor(currentTime / PLAYBACK_SAVE_STEP_SECONDS)

        if (playbackStep === lastSavedPlaybackStep) {
          return
        }

        lastSavedPlaybackStep = playbackStep
        emitPlaybackProgress(currentTime, player.duration() ?? null)
      })

      player.on('pause', () => {
        const currentTime = player.currentTime()
        emitPlaybackProgress(typeof currentTime === 'number' ? currentTime : null, player.duration() ?? null)
      })

      player.on('volumechange', () => {
        emitCurrentPlayerState(player)
      })

      player.on('progress', () => {
        if (isLiveStream(player)) {
          updateLiveProgressBar(player)
        }
      })

      player.on('seeked', () => {
        if (isLiveStream(player)) {
          updateLiveProgressBar(player)
        }
      })

      /**
       * Hides the poster overlay from the video player.
       */
      const hidePosterOverlay = () => {
        isPosterOverlayVisible.value = false
      }

      player.on('play', hidePosterOverlay)
      player.on('playing', () => {
        hidePosterOverlay()
        emitPlaybackStarted(getPlaybackEventSourceUrl(player, props.source.src))
      })

      player.on('ended', () => {
        lastSavedPlaybackStep = -1
        emitPlaybackProgress(null)
        emitPlaybackEnded()
      })

      player.on('dispose', () => {
        playerElement?.removeEventListener('click', handleQualityMenuClick)
        audioTracks?.removeEventListener?.('change', emitPlayerStateAfterUiUpdate)
        textTracks?.removeEventListener?.('change', emitPlayerStateAfterUiUpdate)
        playerElement?.removeEventListener('change', handleTextTrackSettingsChange, true)
        playerElement?.removeEventListener('click', handleTextTrackSettingsClick, true)
      })

      void applySourceToPlayer(player, source, props.initialPlayerState ?? null)
    })
  }

  watch(
    () => videoElement.value,
    (element) => {
      destroyActivePlayer()

      if (!element) {
        return
      }

      createPlayer(element, props.source)
    },
    { immediate: true },
  )

  watch(
    () => props.source,
    (nextSource, previousSource) => {
      if (!activePlayer.value || !previousSource || previousSource.src === nextSource.src) {
        return
      }

      const player = activePlayer.value
      const playerState = capturePlayerState(player, preferredQuality)
      emitPlayerState(playerState)

      const currentTime = player.currentTime()
      const pendingTime = props.preservePlaybackOnSourceSwitch &&
        typeof currentTime === 'number' && Number.isFinite(currentTime) && currentTime > 0
        ? currentTime
        : null
      const pendingAutoplay = props.preservePlaybackOnSourceSwitch &&
        typeof player.paused === 'function' && !player.paused()

      void applySourceToPlayer(player, nextSource, playerState, pendingTime, pendingAutoplay)
    },
  )

  watch(
    [
      () => props.poster,
      () => props.overlayLogoUrl,
      () => props.muted,
      () => props.loop,
      () => props.controls,
      () => props.playsinline,
      () => props.preload,
      () => props.ariaHidden,
      () => props.videoAriaLabel,
      () => props.tabIndex,
    ],
    () => {
      if (!activePlayer.value) {
        return
      }

      syncExistingPlayerConfiguration(activePlayer.value)
    },
  )

  watch(
    () => props.autoplay,
    (nextAutoplay) => {
      if (videoElement.value) {
        videoElement.value.autoplay = nextAutoplay ?? false
      }

      activePlayer.value?.autoplay(nextAutoplay ?? false)
    },
  )

  watch(
    () => props.isExternalLoading,
    (isExternalLoading) => {
      syncBigPlaySuppressionAttribute(isVideoInitialLoading.value || (isExternalLoading ?? false))
    },
  )

  watch(
    [
      () => props.showEpisodeAutoplayToggle,
      () => props.isEpisodeAutoplayEnabled,
    ],
    () => {
      if (!activePlayer.value) {
        return
      }

      syncEpisodeAutoplayControl(activePlayer.value)
    },
  )

  watch(
    [
      () => props.showVideoNavigationControls,
      () => props.hasPreviousVideo,
      () => props.hasNextVideo,
      () => props.previousVideoTitle,
      () => props.nextVideoTitle,
    ],
    () => {
      if (!activePlayer.value) {
        return
      }

      syncPrevNextVideoControl(activePlayer.value)
    },
  )

   onBeforeUnmount(() => {
     destroyActivePlayer()
   })

   return {
     isPosterOverlayVisible,
     shouldRenderPosterOverlay,
     isVideoInitialLoading,
     storyboardPreviewSize,
     vttStoryboardGrid,
   }
}
