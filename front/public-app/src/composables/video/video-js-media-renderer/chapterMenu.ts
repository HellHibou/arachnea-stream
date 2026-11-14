import type { ResolvedVideoChapter } from '@/services/players'
import { t } from '@/i18n'

import { positionControlBarMenu } from './controls'
import type { VideoJsPlayer } from './types'

/** CSS class applied to the chapter menu wrapper control. */
const CHAPTER_MENU_CLASS = 'vjs-chapter-menu'
/** CSS class applied to the chapter menu toggle button. */
const CHAPTER_MENU_BUTTON_CLASS = 'vjs-chapter-menu-button'
/** CSS class applied to every chapter menu item. */
const CHAPTER_MENU_ITEM_CLASS = 'vjs-chapter-menu-item'
/** CSS class applied to the chapter start timecode. */
const CHAPTER_MENU_TIME_CLASS = 'vjs-chapter-menu-time'
/** CSS class applied to the chapter label. */
const CHAPTER_MENU_LABEL_CLASS = 'vjs-chapter-menu-label'
/** CSS class added to the menu while it is displayed. */
const MENU_OPEN_CLASS = 'vjs-lock-showing'
/** CSS class applied by Video.js to the currently selected menu item. */
const MENU_SELECTED_CLASS = 'vjs-selected'
/** Chapter type used for the synthetic ranges created between typed chapters. */
const CONTENT_CHAPTER_TYPE = 'chapter'
/** Minimum uncovered duration, in seconds, that becomes a synthetic chapter. */
const MINIMUM_SYNTHETIC_CHAPTER_SECONDS = 0.5
/** Minimum width of the chapter menu in pixels. */
const CHAPTER_MENU_MIN_WIDTH_PX = 384
/** Maximum width of the chapter menu in pixels. */
const CHAPTER_MENU_MAX_WIDTH_PX = 768
/** Horizontal padding kept between the chapter menu and the player edges. */
const CHAPTER_MENU_EDGE_PADDING_PX = 8

/**
 * Formats a chapter start time as a zero-padded `hh:mm:ss` timecode.
 *
 * @param seconds Chapter start time in seconds.
 * @returns Timecode displayed in the chapter menu.
 */
function formatChapterTimecode(seconds: number): string {
  const totalSeconds = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0
  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const remainingSeconds = totalSeconds % 60

  return [hours, minutes, remainingSeconds]
    .map((value) => String(value).padStart(2, '0'))
    .join(':')
}

/**
 * Resolves the label displayed next to a chapter timecode.
 *
 * Explicit chapter titles win; typed chapters otherwise use their localized
 * type label (`Intro`, `Ad`, ...). Untitled content chapters use the generic
 * chapter label.
 *
 * @param chapter Chapter rendered in the menu.
 * @returns Chapter title or localized type label.
 */
function resolveChapterLabel(chapter: ResolvedVideoChapter): string {
  const title = chapter.title.trim()

  if (title) {
    return title
  }

  const typeLabelKey = `player.chapter.${chapter.type}`
  const typeLabel = t(typeLabelKey)

  return typeLabel === typeLabelKey ? t('player.chapter.chapter') : typeLabel
}

/**
 * Reports whether the chapter list already contains content chapters.
 *
 * @param chapters Ordered chapter list.
 * @returns True when at least one `chapter` entry exists.
 */
function hasContentChapters(chapters: ResolvedVideoChapter[]): boolean {
  return chapters.some((chapter) => chapter.type === CONTENT_CHAPTER_TYPE)
}

/**
 * Builds the chapter list displayed in the menu.
 *
 * When the source only provides typed chapters (`ads`, `intro`, `outro`, ...),
 * uncovered ranges are completed with synthetic content chapters so the menu
 * still covers the whole playback timeline. The trailing gap is only added
 * once the player duration is known.
 *
 * @param chapters Chapters resolved for the active source.
 * @param duration Playback duration in seconds, or `null` when unavailable.
 * @returns Ordered chapter list covering the menu entries.
 */
function buildChapterMenuChapters(
  chapters: ResolvedVideoChapter[],
  duration: number | null,
): ResolvedVideoChapter[] {
  const orderedChapters = chapters
    .filter((chapter) => Number.isFinite(chapter.start) && chapter.end > chapter.start)
    .sort((left, right) => left.start - right.start)

  if (orderedChapters.length === 0 || hasContentChapters(orderedChapters)) {
    return orderedChapters
  }

  const menuChapters = [...orderedChapters]
  let coveredUntil = 0

  const addGapChapter = (start: number, end: number) => {
    if (end - start < MINIMUM_SYNTHETIC_CHAPTER_SECONDS) {
      return
    }

    menuChapters.push({
      start,
      end,
      title: '',
      type: CONTENT_CHAPTER_TYPE,
    })
  }

  for (const chapter of orderedChapters) {
    addGapChapter(coveredUntil, chapter.start)
    coveredUntil = Math.max(coveredUntil, chapter.end)
  }

  if (typeof duration === 'number' && Number.isFinite(duration) && duration > 0) {
    addGapChapter(coveredUntil, duration)
  }

  return menuChapters.sort((left, right) => left.start - right.start)
}

/**
 * Reads the active playback duration, ignoring unavailable or invalid values.
 *
 * @param player Video.js player instance.
 * @returns Duration in seconds, or `null` when unavailable.
 */
function readPlayerDuration(player: VideoJsPlayer): number | null {
  const duration = player.duration()

  return typeof duration === 'number' && Number.isFinite(duration) && duration > 0
    ? duration
    : null
}

/**
 * Installs a control bar button listing the chapters of the active source.
 *
 * The menu shows each chapter start as `hh:mm:ss` with its title or localized
 * type label. Clicking an entry seeks to the start of that chapter. Items are
 * rebuilt every time the menu opens so synthetic gaps can use the duration
 * once the player exposes it. No button is mounted when the source has no
 * chapter at all.
 *
 * @param player Video.js player instance.
 * @param chapters Chapters resolved for the active source.
 */
export function installChapterMenuButton(
  player: VideoJsPlayer,
  chapters: ResolvedVideoChapter[],
): void {
  if (chapters.length === 0) {
    return
  }

  const playerElement = player.el()
  const controlBarElement = playerElement?.querySelector<HTMLElement>('.vjs-control-bar')

  if (!(playerElement instanceof HTMLElement) || !controlBarElement) {
    return
  }

  const wrapper = document.createElement('div')
  wrapper.className = `vjs-menu-button vjs-menu-button-popup ${CHAPTER_MENU_CLASS}`
  wrapper.dataset.menuAlignment = 'right'

  const button = document.createElement('button')
  button.type = 'button'
  button.className = `vjs-control vjs-button ${CHAPTER_MENU_BUTTON_CLASS}`
  button.setAttribute('aria-haspopup', 'true')
  button.setAttribute('aria-expanded', 'false')

  const icon = document.createElement('span')
  icon.className = 'vjs-chapter-menu-icon mdi mdi-format-list-numbered'
  icon.setAttribute('aria-hidden', 'true')
  button.append(icon)

  const menuElement = document.createElement('div')
  menuElement.className = 'vjs-menu'
  const contentElement = document.createElement('ul')
  contentElement.className = 'vjs-menu-content'
  contentElement.setAttribute('role', 'menu')
  menuElement.append(contentElement)
  wrapper.append(button, menuElement)

  /** Chapters currently rendered in the menu. */
  let menuChapters: ResolvedVideoChapter[] = []
  /** Reports whether the menu is currently open. */
  const isOpen = () => menuElement.classList.contains(MENU_OPEN_CLASS)

  const syncMenuLabels = () => {
    const menuLabel = t('player.chapterMenu.label')
    button.setAttribute('aria-label', menuLabel)
    button.setAttribute('title', menuLabel)
  }

  const syncSelectedChapter = () => {
    const currentTime = player.currentTime() ?? 0

    for (const item of Array.from(contentElement.children)) {
      if (!(item instanceof HTMLElement)) {
        continue
      }

      const chapterIndex = Number(item.dataset.chapterIndex)
      const chapter = menuChapters[chapterIndex]
      const isCurrent = chapter
        ? currentTime >= chapter.start && currentTime < chapter.end
        : false

      item.classList.toggle(MENU_SELECTED_CLASS, isCurrent)
      item.setAttribute('aria-current', String(isCurrent))
    }
  }

  /**
   * Creates one chapter menu item.
   *
   * @param chapter Chapter represented by the item.
   * @param chapterIndex Index of the chapter in the rendered list.
   * @returns Focusable menu item.
   */
  const createMenuItem = (chapter: ResolvedVideoChapter, chapterIndex: number) => {
    const item = document.createElement('li')
    item.className = `vjs-menu-item ${CHAPTER_MENU_ITEM_CLASS}`
    item.dataset.chapterIndex = String(chapterIndex)
    item.setAttribute('role', 'menuitem')
    item.setAttribute('tabindex', '0')

    const timeElement = document.createElement('span')
    timeElement.className = CHAPTER_MENU_TIME_CLASS
    timeElement.textContent = formatChapterTimecode(chapter.start)

    const labelElement = document.createElement('span')
    labelElement.className = CHAPTER_MENU_LABEL_CLASS
    labelElement.textContent = resolveChapterLabel(chapter)

    item.append(timeElement, labelElement)
    item.setAttribute(
      'aria-label',
      `${timeElement.textContent} ${labelElement.textContent}`,
    )

    const activate = () => {
      close()
      player.currentTime(chapter.start)
    }

    item.addEventListener('click', (event) => {
      event.preventDefault()
      activate()
    })
    item.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter' && event.key !== ' ') {
        return
      }

      event.preventDefault()
      activate()
    })

    return item
  }

  const renderMenu = () => {
    menuChapters = buildChapterMenuChapters(chapters, readPlayerDuration(player))
    contentElement.replaceChildren(
      ...menuChapters.map((chapter, chapterIndex) => createMenuItem(chapter, chapterIndex)),
    )
    syncSelectedChapter()
  }

  function handleOutsideClick(event: MouseEvent) {
    if (!event.composedPath().includes(wrapper)) {
      close()
    }
  }

  function close() {
    if (!isOpen()) {
      return
    }

    menuElement.classList.remove(MENU_OPEN_CLASS)
    button.setAttribute('aria-expanded', 'false')
    document.removeEventListener('click', handleOutsideClick)

    if (document.activeElement !== button && wrapper.contains(document.activeElement)) {
      button.focus()
    }
  }

  const syncMenuSize = () => {
    const availableWidth = Math.max(
      0,
      playerElement.clientWidth - (CHAPTER_MENU_EDGE_PADDING_PX * 2),
    )
    const availableHeight = Math.max(
      0,
      playerElement.clientHeight - controlBarElement.offsetHeight,
    )

    contentElement.style.minWidth = availableWidth > 0
      ? `${Math.min(CHAPTER_MENU_MIN_WIDTH_PX, availableWidth)}px`
      : ''
    contentElement.style.maxWidth = availableWidth > 0
      ? `${Math.min(CHAPTER_MENU_MAX_WIDTH_PX, availableWidth)}px`
      : ''
    contentElement.style.maxHeight = availableHeight > 0 ? `${availableHeight}px` : ''
  }

  const syncOpenMenuLayout = () => {
    if (!isOpen()) {
      return
    }

    syncMenuSize()
    positionControlBarMenu(playerElement, wrapper, 'right')
  }

  const open = () => {
    syncMenuLabels()
    renderMenu()
    syncMenuSize()
    menuElement.classList.add(MENU_OPEN_CLASS)
    button.setAttribute('aria-expanded', 'true')
    document.addEventListener('click', handleOutsideClick)
    positionControlBarMenu(playerElement, wrapper, 'right')
    contentElement.querySelector<HTMLElement>('.vjs-menu-item')?.focus()
  }

  button.addEventListener('click', () => {
    if (isOpen()) {
      close()
      return
    }

    open()
  })

  wrapper.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') {
      close()
    }
  })

  const syncCurrentChapter = () => {
    if (isOpen()) {
      syncSelectedChapter()
    }
  }

  syncMenuLabels()

  const qualityButton = controlBarElement.querySelector(
    '.vjs-quality-menu-wrapper, .vjs-quality-menu-button',
  )
  const fullscreenButton = controlBarElement.querySelector('.vjs-fullscreen-control')
  const anchorButton = qualityButton ?? fullscreenButton

  if (anchorButton) {
    controlBarElement.insertBefore(wrapper, anchorButton)
  } else {
    controlBarElement.append(wrapper)
  }

  player.on('timeupdate', syncCurrentChapter)
  player.on('seeked', syncCurrentChapter)
  player.on('userinactive', close)
  player.on('fullscreenchange', syncOpenMenuLayout)
  window.addEventListener('resize', syncOpenMenuLayout)

  player.on('dispose', () => {
    document.removeEventListener('click', handleOutsideClick)
    player.off('timeupdate', syncCurrentChapter)
    player.off('seeked', syncCurrentChapter)
    player.off('userinactive', close)
    player.off('fullscreenchange', syncOpenMenuLayout)
    window.removeEventListener('resize', syncOpenMenuLayout)
    wrapper.remove()
  })
}
