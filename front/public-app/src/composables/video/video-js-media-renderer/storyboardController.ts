import { computed, shallowRef } from 'vue'
import type { ResolvedVideoMediaSource } from '@/services/players'
import type { VideoJsPlayer, VideoJsSpriteThumbnailsPlugin } from './types'
import {
  STORYBOARD_PREVIEW_WIDTH,
  STORYBOARD_PREVIEW_HEIGHT,
  STORYBOARD_RESPONSIVE_WIDTH,
  STORYBOARD_TOOLTIP_GAP,
  type StoryboardVttCue,
  type ResolvedSpriteThumbnailOptions,
} from './storyboard'

interface StoryboardControllerOptions {
  getPlayer: () => VideoJsPlayer | null
  getSource: () => ResolvedVideoMediaSource
  hasControls: () => boolean
}

/**
 * Owns one renderer's storyboard plugin, current cues and preview geometry.
 * @param options Readers for the current player, source and control visibility.
 * @returns Storyboard lifecycle operations and reactive preview geometry.
 */
export function createStoryboardController(options: StoryboardControllerOptions) {
  const { getPlayer, getSource, hasControls } = options
  let spriteThumbnailsPlugin: VideoJsSpriteThumbnailsPlugin | null = null
  let storyboardVttCues: StoryboardVttCue[] = []
  const vttStoryboardGrid = shallowRef<{
    width: number; height: number; columns: number; rows: number
  } | null>(null)
  const storyboardPreviewSize = computed(() => {
    // Read the player width so the preview follows the plugin responsive factor
    // applied below 600 px wide players.
    const playerWidth = getPlayer()?.currentWidth() ?? 0
    const grid = vttStoryboardGrid.value
    const storyboard = getSource().storyboard
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
   * Attaches the plugin and cue override once the player is ready.
   * @param player Ready Video.js instance.
   */
  function attach(player: VideoJsPlayer) {
    if (hasControls() && typeof player.spriteThumbnails === 'function') {
      spriteThumbnailsPlugin = player.spriteThumbnails({}) ?? null
    }
    installStoryboardVttCueOverride(player, () => storyboardPreviewSize.value, () => storyboardVttCues)
  }

  /** Clears metadata belonging to the preceding source. */
  function reset() {
    storyboardVttCues = []
    vttStoryboardGrid.value = null
  }

  /**
   * Publishes resolved geometry after the source controller rejects stale loads.
   * @param resolved Storyboard options and cues for the current source.
   */
  function configure(resolved: ResolvedSpriteThumbnailOptions) {
    storyboardVttCues = resolved.cues
    const optWidth = resolved.options.width as number | undefined
    const optHeight = resolved.options.height as number | undefined
    const optColumns = resolved.options.columns as number | undefined
    const optRows = resolved.options.rows as number | undefined
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
  }

  /**
   * Derives missing sprite timing once media duration is available.
   * @param player Player with available metadata.
   * @param source Media source associated with those metadata.
   */
  function syncInterval(player: VideoJsPlayer, source: ResolvedVideoMediaSource) {
    syncDerivedStoryboardInterval(player, source, spriteThumbnailsPlugin)
  }

  /** Releases metadata and the plugin handle after the player is disposed. */
  function dispose() {
    reset()
    spriteThumbnailsPlugin = null
  }

  return { attach, reset, configure, syncInterval, dispose, storyboardPreviewSize, vttStoryboardGrid }
}

export type StoryboardController = ReturnType<typeof createStoryboardController>

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
 * @param getPreviewSize Reads the current resolved storyboard preview size.
 * @param getVttCues Reads the exact VTT cue geometry for the current source.
 */
function installStoryboardVttCueOverride(
  player: VideoJsPlayer,
  getPreviewSize: () => {
    displayWidth: number
    displayHeight: number
    columns: number
    fitScale: number
    responsiveScale: number
    displayToSourceScale: number
  } | null,
  getVttCues: () => StoryboardVttCue[],
): void {
  const playerElement = player.el()
  const seekBarElement = playerElement?.querySelector<HTMLElement>('.vjs-progress-control')

  if (!playerElement || !seekBarElement) {
    return
  }

  const handleMouseMove = (event: MouseEvent) => {
    // The plugin writes its geometry synchronously during this event. Apply one
    // override afterwards, starting from plugin pixels or raw VTT pixels.
    queueMicrotask(() => {
      const previewSize = getPreviewSize()
      const tooltip = playerElement.querySelector<HTMLElement>('.vjs-mouse-display .vjs-time-tooltip')
      if (!previewSize || !tooltip) {
        return
      }

      const pluginWidth = Number.parseFloat(tooltip.style.width)
      const pluginHeight = Number.parseFloat(tooltip.style.height)
      if (!(pluginWidth > 0) || !(pluginHeight > 0)) {
        return
      }

      const cues = getVttCues()
      const duration = player.duration()
      const seekBar = playerElement.querySelector<HTMLElement>('.vjs-progress-holder')
      const bounds = seekBar?.getBoundingClientRect()
      const progress = bounds && bounds.width > 0
        ? Math.min(1, Math.max(0, (event.clientX - bounds.left) / bounds.width))
        : null
      const time = progress !== null && typeof duration === 'number' && Number.isFinite(duration)
        ? progress * duration
        : null
      const cue = time !== null
        ? cues.find(({ start, end }) => time >= start && time < end)
        ?? (time === duration ? cues[cues.length - 1] : undefined)
        : undefined

      tooltip.style.width = `${previewSize.displayWidth}px`
      tooltip.style.height = `${previewSize.displayHeight}px`
      tooltip.style.top = `${-(previewSize.displayHeight + STORYBOARD_TOOLTIP_GAP)}px`

      if (cue) {
        tooltip.style.backgroundImage = `url("${cue.imageUrl}")`
        tooltip.style.backgroundPosition = `${-cue.x * previewSize.displayToSourceScale}px ${-cue.y * previewSize.displayToSourceScale}px`
      } else {
        // Plugin positions already include its responsive scale; only the fit
        // scale remains to be applied for a regular sprite storyboard.
        tooltip.style.backgroundPosition = tooltip.style.backgroundPosition
          .split(' ')
          .map((component) => {
            const value = Number.parseFloat(component)
            return Number.isFinite(value) ? `${value * previewSize.fitScale}px` : component
          })
          .join(' ')
      }

      if (previewSize.columns > 0) {
        tooltip.style.backgroundSize = `${previewSize.displayWidth * previewSize.columns}px auto`
      }
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
 * @param spriteThumbnailsPlugin Sprite thumbnail plugin instance attached to the active player.
 */
function syncDerivedStoryboardInterval(
  player: VideoJsPlayer,
  source: ResolvedVideoMediaSource,
  spriteThumbnailsPlugin: VideoJsSpriteThumbnailsPlugin | null,
): void {
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
