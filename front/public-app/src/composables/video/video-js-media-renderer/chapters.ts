import type { ResolvedVideoChapter } from '@/services/players'
import type { VideoJsPlayer } from '@/composables/video/video-js-media-renderer/types'
import { t } from '@/i18n'

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
/** CSS class applied to the chapter overlay when it floats above the timecode. */
const CHAPTER_OVERLAY_FLOATING_CLASS = 'vjs-chapter-overlay--floating'
/** CSS class prefix applied to skip chapter buttons. */
const SKIP_CHAPTER_BUTTON_CLASS_PREFIX = 'vjs-skip-'

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

    overlay.textContent = chapter.title || (chapter.type === 'chapter' ? null : t('player.chapter.' + chapter.type))
    overlay.classList.add(CHAPTER_TITLE_VISIBLE_CLASS)

    // Align the overlay with the current time tooltip position.
    const timeTooltip = playerElement.querySelector<HTMLElement>('.vjs-time-tooltip')
    if (!timeTooltip) {
      return
    }

    const tooltipRect = timeTooltip.getBoundingClientRect()
    const progressRect = progressControl.getBoundingClientRect()
    const left = tooltipRect.left - progressRect.left + tooltipRect.width / 2
    const top = tooltipRect.top - progressRect.top

    if (timeTooltip.querySelector('.vjs-thumbnail')) {
      // With a sprite thumbnail the tooltip is wide enough to host the title.
      overlay.classList.remove(CHAPTER_OVERLAY_FLOATING_CLASS)
      overlay.style.left = `${left}px`
      overlay.style.top = `${top}px`
      overlay.style.width = `${tooltipRect.width}px`
      return
    }

    // Without a thumbnail the tooltip only shows the timecode; float the title
    // above it with a content-based width so neither is truncated or masked.
    overlay.classList.add(CHAPTER_OVERLAY_FLOATING_CLASS)
    overlay.style.width = ''
    const halfWidth = overlay.offsetWidth / 2
    const clampedCenter = Math.min(
      Math.max(left, halfWidth),
      Math.max(halfWidth, progressRect.width - halfWidth),
    )
    overlay.style.left = `${clampedCenter}px`
    overlay.style.top = `${top}px`
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
  if (chapters.length === 0) {
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

      // Draw a divider at the chapter start boundary.
      if (leftPct > 0) {
        const startDivider = document.createElement('div')
        startDivider.className = CHAPTER_SEGMENT_DIVIDER_CLASS
        startDivider.style.left = `${leftPct}%`
        container.appendChild(startDivider)
      }

      // Draw a divider at the chapter end boundary.
      if (rightPct < 100) {
        const endDivider = document.createElement('div')
        endDivider.className = CHAPTER_SEGMENT_DIVIDER_CLASS
        endDivider.style.left = `${rightPct}%`
        container.appendChild(endDivider)
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

/**
 * Chapter types that can be skipped with a dedicated button.
 */
export type SkipChapterType = 'intro' | 'outro' | 'ads'

/** i18n key of the label used by each skip button type. */
const SKIP_CHAPTER_LABEL_KEYS: Record<SkipChapterType, string> = {
  intro: 'player.chapter.skipIntro',
  outro: 'player.chapter.skipOutro',
  ads: 'player.chapter.skipAds',
}

/**
 * Installs a button that appears during chapters of the given type and seeks
 * to the end of the current chapter on click.
 *
 * A type may cover several chapters (for example every advertising period of
 * a DASH multiperiod stream); the button stays visible across consecutive
 * chapters and always skips the one being played.
 *
 * @param player Video.js player instance.
 * @param chapters Ordered list of chapters from the resolved stream.
 * @param chapterType Type of chapter that the button skips.
 */
export function installSkipChapterButton(
  player: VideoJsPlayer,
  chapters: ResolvedVideoChapter[],
  chapterType: SkipChapterType,
): void {
  const typeChapters = chapters.filter((item) => item.type === chapterType)
  if (typeChapters.length === 0) {
    return
  }

  const playerElement = player.el()
  if (!playerElement) {
    return
  }

  const findCurrentChapter = () => {
    const currentTime = player.currentTime() ?? 0
    return findChapterAtTime(typeChapters, currentTime)
  }

  const button = document.createElement('button')
  const buttonClass = `${SKIP_CHAPTER_BUTTON_CLASS_PREFIX}${chapterType}-button`
  const visibleButtonClass = `${buttonClass}--visible`
  button.className = buttonClass
  button.textContent = t(SKIP_CHAPTER_LABEL_KEYS[chapterType])
  button.addEventListener('click', () => {
    const chapter = findCurrentChapter() ?? typeChapters[typeChapters.length - 1]
    if (!chapter) {
      return
    }

    const duration = player.duration() ?? chapter.end
    const targetTime = Math.min(chapter.end, duration - 0.5)
    player.currentTime(targetTime)
  })

  const handleTimeUpdate = () => {
    button.classList.toggle(visibleButtonClass, findCurrentChapter() !== null)
  }

  player.on('timeupdate', handleTimeUpdate)
  player.on('loadedmetadata', handleTimeUpdate)
  player.on('seeked', handleTimeUpdate)

  playerElement.appendChild(button)

  player.on('dispose', () => {
    player.off('timeupdate', handleTimeUpdate)
    player.off('loadedmetadata', handleTimeUpdate)
    player.off('seeked', handleTimeUpdate)
    button.remove()
  })
}
