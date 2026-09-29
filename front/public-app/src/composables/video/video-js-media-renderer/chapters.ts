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
/** CSS class applied to the skip chapter button label. */
const SKIP_CHAPTER_BUTTON_LABEL_CLASS = 'vjs-skip-chapter-button-label'
/** Material Design icon shown after the skip chapter button label. */
const SKIP_CHAPTER_BUTTON_ICON = 'mdi mdi-skip-forward'
/** CSS class applied to the skip chapter button icon. */
const SKIP_CHAPTER_BUTTON_ICON_CLASS = 'vjs-skip-chapter-button-icon'
/** CSS class suffix added to a skip chapter button while the player control bar is hidden. */
const SKIP_CHAPTER_BUTTON_CONTROLS_HIDDEN_CLASS_SUFFIX = '--controls-hidden'
/**
 * Seconds a skip chapter button stays visible after its chapter starts while the player control bar
 * is hidden, since the button otherwise fades out together with the controls.
 */
const SKIP_CHAPTER_BUTTON_HIDDEN_CONTROLS_REVEAL_SECONDS = 5
/**
 * Chapter types that display a dedicated skip button.
 */
export const SKIP_CHAPTER_BUTTON_TYPES = [
  'previously',
  'intro',
  'coming_next',
  'outro',
  'ads',
] as const

/**
 * Chapter types that can be skipped with a dedicated button.
 */
export type SkipChapterType = (typeof SKIP_CHAPTER_BUTTON_TYPES)[number]

/**
 * Skip button chapter types that stay visible for the whole chapter even
 * while the player control bar is hidden.
 */
export const SKIP_CHAPTER_BUTTON_ALWAYS_VISIBLE_TYPES: ReadonlySet<SkipChapterType> = new Set([
  'ads',
])
/**
 * Player class added by Video.js as soon as playback starts for the current source, at the same
 * moment the poster/title image is hidden.
 */
const PLAYBACK_STARTED_CLASS = 'vjs-has-started'

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

/** i18n key of the label used by each skip button type. */
const SKIP_CHAPTER_LABEL_KEYS: Record<SkipChapterType, string> = {
  intro: 'player.chapter.skipIntro',
  previously: 'player.chapter.skipPreviously',
  coming_next: 'player.chapter.skipComingNext',
  outro: 'player.chapter.skipOutro',
  ads: 'player.chapter.skipAds',
}

/**
 * Options accepted by the automatic chapter skip installer.
 */
export interface AutoSkipChaptersOptions {
  /**
   * Chapters eligible for automatic skipping, typically the merged chapter list.
   */
  chapters: ResolvedVideoChapter[]
  /**
   * Reads whether automatic skipping is enabled for a chapter type.
   */
  isAutoskipEnabled: (chapterType: SkipChapterType) => boolean
}

/**
 * Merges contiguous or overlapping chapters of the same type into single ranges.
 *
 * Collapsing a whole ad pod into one range lets the auto-skip jump over the
 * series in a single seek instead of skipping pod segments one by one.
 *
 * @param chapters Ordered list of chapters from the resolved stream.
 * @param gapToleranceSeconds Maximum gap between two same-type chapters merged together.
 * @returns Merged chapter ranges ordered by start time.
 */
export function mergeContiguousChapters(
  chapters: ResolvedVideoChapter[],
  gapToleranceSeconds = 0.5,
): ResolvedVideoChapter[] {
  const sorted = [...chapters].sort((a, b) => a.start - b.start)
  const merged: ResolvedVideoChapter[] = []

  for (const chapter of sorted) {
    const previous = merged[merged.length - 1]

    if (
      previous &&
      previous.type === chapter.type &&
      chapter.start <= previous.end + gapToleranceSeconds
    ) {
      previous.end = Math.max(previous.end, chapter.end)
      if (!previous.title && chapter.title) {
        previous.title = chapter.title
      }
      continue
    }

    merged.push({ ...chapter })
  }

  return merged
}

/**
 * Installs automatic skipping of skippable chapters.
 *
 * Each skippable chapter type carries its own `videoPlayer.autoskip.*`
 * parameter: the current chapter is skipped as soon as playback enters it
 * when its parameter is enabled. Advertising is skipped unconditionally;
 * other chapter types are only skipped automatically while episode autoplay
 * is enabled (gated by the caller through `isAutoskipEnabled`). Contiguous
 * chapters of the same type are merged beforehand so a whole advertising pod
 * is jumped over with a single seek.
 *
 * @param player Video.js player instance.
 * @param options Auto-skip configuration.
 */
export function installAutoSkipChapters(player: VideoJsPlayer, options: AutoSkipChaptersOptions): void {
  const skippableTypes = new Set<SkipChapterType>(SKIP_CHAPTER_BUTTON_TYPES)
  const merged = mergeContiguousChapters(
    options.chapters.filter((chapter) => skippableTypes.has(chapter.type as SkipChapterType)),
  )

  if (merged.length === 0) {
    return
  }

  const skipCurrentChapter = () => {
    const currentTime = player.currentTime() ?? 0
    const chapter = findChapterAtTime(merged, currentTime)

    if (!chapter || !options.isAutoskipEnabled(chapter.type as SkipChapterType)) {
      return
    }

    // A seek to the range end lands inside the next contiguous range when the
    // source reports back-to-back ranges; jump over the whole merged range.
    if (currentTime >= chapter.end - 0.25) {
      return
    }

    const duration = player.duration() ?? chapter.end
    const targetTime = Math.min(chapter.end, duration - 0.5)

    if (targetTime > currentTime) {
      player.currentTime(targetTime)
    }
  }

  player.on('timeupdate', skipCurrentChapter)
  player.on('seeked', skipCurrentChapter)
  player.on('loadedmetadata', skipCurrentChapter)
  player.on('play', skipCurrentChapter)
  player.on('playing', skipCurrentChapter)

  player.on('dispose', () => {
    player.off('timeupdate', skipCurrentChapter)
    player.off('seeked', skipCurrentChapter)
    player.off('loadedmetadata', skipCurrentChapter)
    player.off('play', skipCurrentChapter)
    player.off('playing', skipCurrentChapter)
  })
}

/**
 * Installs a button that appears during chapters of the given type and seeks
 * to the end of the current chapter on click.
 *
 * A type may cover several chapters (for example every advertising period of
 * a DASH multiperiod stream); the button stays visible across consecutive
 * chapters and always skips the one being played.
 *
 * Visibility follows the player control bar: while the control bar is shown
 * the button stays available for the whole chapter, while it is hidden
 * (playing with an inactive user) the button is only revealed during the
 * first {@link SKIP_CHAPTER_BUTTON_HIDDEN_CONTROLS_REVEAL_SECONDS} seconds of
 * the chapter and is anchored to the player bottom instead of sitting above
 * the control bar. Chapter types listed in
 * {@link SKIP_CHAPTER_BUTTON_ALWAYS_VISIBLE_TYPES} (advertising) stay visible
 * for the whole chapter even while the control bar is hidden.
 *
 * The button also stays hidden until playback has started for the current
 * source, mirroring how the poster/title image is hidden on the first play.
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

  /**
   * Reports whether the control bar is hidden, mirroring the Video.js rule that
   * fades it out while playing and the user is inactive.
   */
  const isControlBarHidden = () =>
    playerElement.classList.contains('vjs-playing') &&
    playerElement.classList.contains('vjs-user-inactive')

  const button = document.createElement('button')
  const buttonClass = `${SKIP_CHAPTER_BUTTON_CLASS_PREFIX}${chapterType}-button`
  const visibleButtonClass = `${buttonClass}--visible`
  const controlsHiddenButtonClass = `${buttonClass}${SKIP_CHAPTER_BUTTON_CONTROLS_HIDDEN_CLASS_SUFFIX}`
  button.className = buttonClass
  const buttonLabel = document.createElement('span')
  buttonLabel.className = SKIP_CHAPTER_BUTTON_LABEL_CLASS
  buttonLabel.textContent = t(SKIP_CHAPTER_LABEL_KEYS[chapterType])
  const buttonIcon = document.createElement('span')
  buttonIcon.className = `${SKIP_CHAPTER_BUTTON_ICON_CLASS} ${SKIP_CHAPTER_BUTTON_ICON}`
  buttonIcon.setAttribute('aria-hidden', 'true')
  button.append(buttonLabel, buttonIcon)
  button.addEventListener('click', () => {
    const chapter = findCurrentChapter() ?? typeChapters[typeChapters.length - 1]
    if (!chapter) {
      return
    }

    const duration = player.duration() ?? chapter.end
    const targetTime = Math.min(chapter.end, duration - 0.5)
    player.currentTime(targetTime)
  })

  const syncButtonState = () => {
    const chapter = findCurrentChapter()
    const isControlBarVisible = !isControlBarHidden()
    const elapsedInChapter = chapter ? (player.currentTime() ?? chapter.start) - chapter.start : 0
    const isWithinRevealWindow =
      isControlBarVisible ||
      SKIP_CHAPTER_BUTTON_ALWAYS_VISIBLE_TYPES.has(chapterType) ||
      elapsedInChapter < SKIP_CHAPTER_BUTTON_HIDDEN_CONTROLS_REVEAL_SECONDS
    const hasPlaybackStarted = playerElement.classList.contains(PLAYBACK_STARTED_CLASS)

    button.classList.toggle(
      visibleButtonClass,
      chapter !== null && hasPlaybackStarted && isWithinRevealWindow,
    )
    button.classList.toggle(controlsHiddenButtonClass, !isControlBarVisible)
  }

  player.on('timeupdate', syncButtonState)
  player.on('loadedmetadata', syncButtonState)
  player.on('seeked', syncButtonState)
  player.on('useractive', syncButtonState)
  player.on('userinactive', syncButtonState)
  player.on('play', syncButtonState)
  player.on('playing', syncButtonState)
  player.on('pause', syncButtonState)
  player.on('loadstart', syncButtonState)

  playerElement.appendChild(button)
  syncButtonState()

  player.on('dispose', () => {
    player.off('timeupdate', syncButtonState)
    player.off('loadedmetadata', syncButtonState)
    player.off('seeked', syncButtonState)
    player.off('useractive', syncButtonState)
    player.off('userinactive', syncButtonState)
    player.off('play', syncButtonState)
    player.off('playing', syncButtonState)
    player.off('pause', syncButtonState)
    player.off('loadstart', syncButtonState)
    button.remove()
  })
}
