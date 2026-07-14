import type { ResolvedVideoChapter } from '@/services/players'
import type { VideoJsPlayer } from '@/composables/video/video-js-media-renderer/types'

/** CSS class applied to the chapter overlay element. */
const CHAPTER_TITLE_CLASS = 'vjs-chapter-overlay'
/** CSS class added when a chapter title should be visible. */
const CHAPTER_TITLE_VISIBLE_CLASS = 'vjs-chapter-overlay--visible'
/** CSS class applied to the chapter segment container. */
const CHAPTER_SEGMENTS_CLASS = 'vjs-chapter-segments'
/** CSS class applied to each chapter segment element. */
const CHAPTER_SEGMENT_CLASS = 'vjs-chapter-segment'
/** CSS class applied to the chapter segment divider. */
const CHAPTER_SEGMENT_DIVIDER_CLASS = 'vjs-chapter-segment-divider'

/**
 * Returns the active duration when available and finite.
 */
function getDuration(player: VideoJsPlayer): number | null {
  const d = player.duration()
  return typeof d === 'number' && Number.isFinite(d) && d > 0 ? d : null
}

/**
 * Installs a chapter title overlay on the progress bar.
 *
 * When hovering over the progress control, the overlay shows the title of
 * the chapter that contains the current hover time position. The overlay is
 * positioned independently of the sprite thumbnail tooltip to avoid
 * conflicts with Video.js's `TimeTooltip` which overwrites child elements
 * via `textContent`.
 *
 * @param player Video.js player instance.
 * @param chapters Ordered list of chapters from the resolved stream.
 */
export function installChapterOverlay(player: VideoJsPlayer, chapters: ResolvedVideoChapter[]): void {
  if (chapters.length === 0) {
    return
  }

  const playerElement = player.el()
  if (!playerElement) {
    return
  }

  const progressControl = playerElement.querySelector<HTMLElement>('.vjs-progress-control')
  if (!progressControl) {
    return
  }

  const overlay = document.createElement('div')
  overlay.className = CHAPTER_TITLE_CLASS
  progressControl.appendChild(overlay)

  const handleMouseMove = (event: MouseEvent) => {
    const seekBar = progressControl.querySelector<HTMLElement>('.vjs-progress-holder')
    if (!seekBar) {
      hideOverlay(overlay)
      return
    }

    const duration = getDuration(player)
    if (!duration) {
      hideOverlay(overlay)
      return
    }

    const seekBarRect = seekBar.getBoundingClientRect()
    const xRatio = (event.clientX - seekBarRect.left) / seekBarRect.width
    const clampedRatio = Math.max(0, Math.min(1, xRatio))
    const hoverTime = clampedRatio * duration

    const chapter = findChapterAtTime(chapters, hoverTime)
    if (!chapter) {
      hideOverlay(overlay)
      return
    }

    // Align the overlay with the current time tooltip position.
    const timeTooltip = playerElement.querySelector<HTMLElement>('.vjs-time-tooltip')
    if (timeTooltip) {
      const tooltipRect = timeTooltip.getBoundingClientRect()
      const progressRect = progressControl.getBoundingClientRect()
      const left = tooltipRect.left - progressRect.left + tooltipRect.width / 2
      const top = tooltipRect.top - progressRect.top
      overlay.style.left = `${left}px`
      overlay.style.top = `${top}px`
      overlay.style.width = `${tooltipRect.width}px`
    }

    overlay.textContent = chapter.title
    overlay.classList.add(CHAPTER_TITLE_VISIBLE_CLASS)
  }

  const handleMouseLeave = () => {
    hideOverlay(overlay)
  }

  progressControl.addEventListener('mousemove', handleMouseMove)
  progressControl.addEventListener('mouseleave', handleMouseLeave)

  player.on('dispose', () => {
    progressControl.removeEventListener('mousemove', handleMouseMove)
    progressControl.removeEventListener('mouseleave', handleMouseLeave)
    overlay.remove()
  })
}

/**
 * Installs chapter segment dividers on the progress bar.
 *
 * Each chapter is represented as a visual segment on the seek bar,
 * with a subtle divider at the boundary between consecutive chapters.
 * Segments are re-rendered when the player duration becomes available.
 *
 * @param player Video.js player instance.
 * @param chapters Ordered list of chapters from the resolved stream.
 */
export function installChapterSegments(player: VideoJsPlayer, chapters: ResolvedVideoChapter[]): void {
  if (chapters.length < 2) {
    return
  }

  const playerElement = player.el()
  if (!playerElement) {
    return
  }

  const seekBar = playerElement.querySelector<HTMLElement>('.vjs-progress-holder')
  if (!seekBar) {
    return
  }

  const renderSegments = () => {
    removeExistingSegments(seekBar)

    const duration = getDuration(player)
    if (!duration) {
      return
    }

    const lastChapter = chapters[chapters.length - 1]
    if (!lastChapter) {
      return
    }

    const container = document.createElement('div')
    container.className = CHAPTER_SEGMENTS_CLASS

    for (const chapter of chapters) {
      const leftPct = Math.max(0, (chapter.start / duration) * 100)
      const rightPct = Math.min(100, (chapter.end / duration) * 100)

      if (rightPct <= leftPct) {
        continue
      }

      const segment = document.createElement('div')
      segment.className = CHAPTER_SEGMENT_CLASS
      segment.style.left = `${leftPct}%`
      segment.style.width = `${rightPct - leftPct}%`
      container.appendChild(segment)

      // Add a divider at the chapter boundary (except for the last chapter).
      if (chapter.end < lastChapter.end) {
        const divider = document.createElement('div')
        divider.className = CHAPTER_SEGMENT_DIVIDER_CLASS
        divider.style.left = `${rightPct}%`
        container.appendChild(divider)
      }
    }

    seekBar.appendChild(container)
  }

  renderSegments()

  player.on('durationchange', renderSegments)
  player.on('dispose', () => {
    player.off('durationchange', renderSegments)
    removeExistingSegments(seekBar)
  })
}

function removeExistingSegments(seekBar: HTMLElement): void {
  const existing = seekBar.querySelector<HTMLElement>(`.${CHAPTER_SEGMENTS_CLASS}`)
  if (existing) {
    existing.remove()
  }
}

function hideOverlay(overlay: HTMLElement): void {
  overlay.classList.remove(CHAPTER_TITLE_VISIBLE_CLASS)
  overlay.textContent = ''
}

function findChapterAtTime(chapters: ResolvedVideoChapter[], time: number): ResolvedVideoChapter | null {
  return chapters.find((chapter) => time >= chapter.start && time < chapter.end) ?? null
}
