import {
  EPISODE_AUTOPLAY_CONTROL_ACTIVE_CLASS,
  EPISODE_AUTOPLAY_CONTROL_CLASS,
  NEXT_VIDEO_CONTROL_CLASS,
  PREV_VIDEO_CONTROL_CLASS,
  REMAINING_TIME_CLASS,
} from '@/composables/video/video-js-media-renderer/constants'
import {
  getLiveSeekableRange,
  isDurationAvailable,
  isLiveStream,
  seekInLiveStream,
} from '@/composables/video/video-js-media-renderer/live'
import { syncTimerToggleState } from '@/composables/video/video-js-media-renderer/state'
import type { VideoJsPlayer } from '@/composables/video/video-js-media-renderer/types'
import { t } from '@/i18n'

/** Options for installing the timer toggle control. */
interface InstallTimerToggleOptions {
  /** Whether player controls are enabled. */
  controls: boolean
  /** Callback invoked when the timer toggle state changes. */
  onStateChange: () => void
}

/** Options for installing seek-on-click behavior. */
interface InstallSeekOnClickOptions {
  /** Whether player controls are enabled. */
  controls: boolean
}

/** Padding in pixels from the edges of the control bar for menu positioning. */
const CONTROL_BAR_MENU_EDGE_PADDING_PX = 8

/** Options for syncing episode autoplay toggle control. */
interface SyncEpisodeAutoplayToggleControlOptions {
  /** Whether player controls are enabled. */
  controls: boolean
  /** Whether the episode autoplay toggle should be shown. */
  showEpisodeAutoplayToggle: boolean
  /** Whether episode autoplay is currently enabled. */
  isEpisodeAutoplayEnabled: boolean
  /** Callback invoked when the toggle is clicked. */
  onEpisodeAutoplayToggle: () => void
}

/**
 * Clamps a value between minimum and maximum bounds.
 *
 * @param value - Value to clamp.
 * @param min - Minimum allowed value.
 * @param max - Maximum allowed value.
 * @returns Clamped value.
 */
function clampMenuLeftOffset(value: number, min: number, max: number): number {
  if (max < min) {
    return min
  }

  return Math.min(Math.max(value, min), max)
}

/**
 * Gets the episode autoplay toggle button element from the player.
 *
 * @param player - Video.js player instance.
 * @returns Episode autoplay toggle button element or null.
 */
function getEpisodeAutoplayToggleElement(player: VideoJsPlayer): HTMLButtonElement | null {
  return player.el()?.querySelector<HTMLButtonElement>(`.${EPISODE_AUTOPLAY_CONTROL_CLASS}`) ?? null
}

/**
 * Determines whether a menu button should have its menu positioned.
 *
 * @param menuButtonElement - Menu button element to check.
 * @returns True when the menu should be positioned.
 */
function shouldPositionControlBarMenu(menuButtonElement: HTMLElement): boolean {
  return (
    !menuButtonElement.classList.contains('vjs-quality-menu-button') &&
    !menuButtonElement.classList.contains('vjs-quality-menu-wrapper') &&
    !menuButtonElement.closest('.vjs-quality-menu-wrapper')
  )
}

/**
 * Gets all menu buttons in the player that should have their menus positioned.
 *
 * @param playerElement - Root player element.
 * @returns Array of menu button elements.
 */
function getPositionableControlBarMenuButtons(playerElement: HTMLElement): HTMLElement[] {
  const menuButtonElements = playerElement.querySelectorAll<HTMLElement>('.vjs-menu-button-popup')
  return Array.from(menuButtonElements).filter(shouldPositionControlBarMenu)
}

/**
 * Positions a control bar menu relative to its button.
 *
 * @param playerElement - Root player element.
 * @param menuButtonElement - Menu button element to position.
 */
function positionControlBarMenu(playerElement: HTMLElement, menuButtonElement: HTMLElement) {
  const menuElement = menuButtonElement.querySelector<HTMLElement>(':scope > .vjs-menu')
  const menuContentElement = menuElement?.querySelector<HTMLElement>('.vjs-menu-content')

  if (!menuElement || !menuContentElement || menuElement.getClientRects().length === 0) {
    return
  }

  const playerRect = playerElement.getBoundingClientRect()
  const buttonRect = menuButtonElement.getBoundingClientRect()
  const contentRect = menuContentElement.getBoundingClientRect()
  const maximumMenuWidth = Math.max(playerRect.width - (CONTROL_BAR_MENU_EDGE_PADDING_PX * 2), 0)
  const naturalMenuWidth = Math.ceil(Math.max(contentRect.width, menuContentElement.scrollWidth))
  const menuWidth = Math.min(naturalMenuWidth, maximumMenuWidth)

  if (menuWidth <= 0) {
    return
  }

  const minimumLeft = playerRect.left + CONTROL_BAR_MENU_EDGE_PADDING_PX
  const maximumLeft = playerRect.right - CONTROL_BAR_MENU_EDGE_PADDING_PX - menuWidth
  const centeredLeft = buttonRect.left + ((buttonRect.width - menuWidth) / 2)
  const menuLeft = clampMenuLeftOffset(centeredLeft, minimumLeft, maximumLeft)

  menuElement.style.left = `${Math.round(menuLeft - buttonRect.left)}px`
  menuElement.style.right = 'auto'
  menuElement.style.width = `${menuWidth}px`
}

/**
 * Synchronizes the visual state of the episode autoplay toggle button.
 *
 * @param button - Episode autoplay toggle button element.
 * @param isEpisodeAutoplayEnabled - Whether episode autoplay is currently enabled.
 */
function syncEpisodeAutoplayToggleState(button: HTMLButtonElement, isEpisodeAutoplayEnabled: boolean) {
  const nextTitle = isEpisodeAutoplayEnabled ? t('player.disableAutoplay') : t('player.enableAutoplay')

  button.classList.toggle(EPISODE_AUTOPLAY_CONTROL_ACTIVE_CLASS, isEpisodeAutoplayEnabled)
  button.setAttribute('aria-pressed', String(isEpisodeAutoplayEnabled))
  button.setAttribute('aria-label', nextTitle)
  button.setAttribute('title', nextTitle)
}

/**
 * Mounts or updates the autoplay toggle inside the Video.js control bar.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param options Reactive configuration and callback used by the control.
 */
export function syncEpisodeAutoplayToggleControl(
  player: VideoJsPlayer,
  options: SyncEpisodeAutoplayToggleControlOptions,
) {
  if (!options.controls) {
    return
  }

  const playerElement = player.el()

  if (!(playerElement instanceof HTMLElement)) {
    return
  }

  const controlBarElement = playerElement.querySelector<HTMLElement>('.vjs-control-bar')

  if (!controlBarElement) {
    return
  }

  const existingSpacer = controlBarElement.querySelector('.arachnea-videojs-div-control')

  if (!existingSpacer) {
    const spacer = document.createElement('div')
    spacer.className = 'arachnea-videojs-div-control'
    const durationElement = controlBarElement.querySelector('.vjs-duration')

    if (durationElement) {
      controlBarElement.insertBefore(spacer, durationElement.nextSibling)
    } else {
      controlBarElement.append(spacer)
    }
  }

  const existingButton = getEpisodeAutoplayToggleElement(player)

  if (!options.showEpisodeAutoplayToggle) {
    existingButton?.remove()
    return
  }

  const button = existingButton ?? document.createElement('button')

  if (!existingButton) {
    button.type = 'button'
    button.className = `vjs-control vjs-button ${EPISODE_AUTOPLAY_CONTROL_CLASS}`
    button.innerHTML = '<span class="vjs-episode-autoplay-track"><span class="vjs-episode-autoplay-thumb"><span class="vjs-episode-autoplay-icon"></span></span></span>'
    button.addEventListener('click', options.onEpisodeAutoplayToggle)

    const qualityButton = controlBarElement.querySelector(
      '.vjs-quality-menu-wrapper, .vjs-quality-menu-button',
    )

    if (qualityButton) {
      controlBarElement.insertBefore(button, qualityButton)
    } else {
      const fullscreenButton = controlBarElement.querySelector('.vjs-fullscreen-control')

      if (fullscreenButton) {
        controlBarElement.insertBefore(button, fullscreenButton)
        syncEpisodeAutoplayToggleState(button, options.isEpisodeAutoplayEnabled)
        return
      }

      controlBarElement.append(button)
    }
  }

  syncEpisodeAutoplayToggleState(button, options.isEpisodeAutoplayEnabled)
}

/**
 * Gets the previous video control button element from the player.
 *
 * @param player - Video.js player instance.
 * @returns Previous video control button element or null.
 */
function getPrevVideoControlElement(player: VideoJsPlayer): HTMLButtonElement | null {
  return player.el()?.querySelector<HTMLButtonElement>(`.${PREV_VIDEO_CONTROL_CLASS}`) ?? null
}

/**
 * Gets the next video control button element from the player.
 *
 * @param player - Video.js player instance.
 * @returns Next video control button element or null.
 */
function getNextVideoControlElement(player: VideoJsPlayer): HTMLButtonElement | null {
  return player.el()?.querySelector<HTMLButtonElement>(`.${NEXT_VIDEO_CONTROL_CLASS}`) ?? null
}

/**
 * Options for syncing prev/next video controls.
 */
interface SyncPrevNextVideoControlsOptions {
  /** Whether player controls are enabled. */
  controls: boolean
  /** Whether the previous video control should be shown. */
  showPrevVideoControl: boolean
  /** Whether the next video control should be shown. */
  showNextVideoControl: boolean
  /** Whether the previous video control is disabled. */
  hasPreviousVideo: boolean
  /** Whether the next video control is disabled. */
  hasNextVideo: boolean
  /** Title displayed when hovering the previous video control. */
  previousVideoTitle: string | null
  /** Title displayed when hovering the next video control. */
  nextVideoTitle: string | null
  /** Callback invoked when the previous video button is clicked. */
  onPrevVideo: () => void
  /** Callback invoked when the next video button is clicked. */
  onNextVideo: () => void
}

/**
 * Synchronizes the visual state of a prev/next video control button.
 *
 * @param button - Control button element.
 * @param isHidden - Whether the button should be hidden.
 * @param labelKey - Translation key for the aria-label.
 */
function syncPrevNextVideoControlState(
  button: HTMLButtonElement | null,
  isHidden: boolean,
  labelKey: string,
  videoTitle: string | null,
) {
  if (!button) {
    return
  }
  button.disabled = isHidden
  button.classList.toggle('vjs-prev-video-control--hidden', isHidden && button.classList.contains('vjs-prev-video-control'))
  button.classList.toggle('vjs-next-video-control--hidden', isHidden && button.classList.contains('vjs-next-video-control'))
  button.setAttribute('aria-label', t(labelKey))
  button.title = videoTitle?.trim() || t(labelKey)
}

/**
 * Mounts or updates prev/next video controls inside the Video.js control bar.
 * These controls are used for navigating between adjacent playable entries (episodes).
 *
 * @param player Video.js player currently bound to the renderer.
 * @param options Reactive configuration and callbacks used by the controls.
 */
export function syncPrevNextVideoControls(
  player: VideoJsPlayer,
  options: SyncPrevNextVideoControlsOptions,
) {
  if (!options.controls) {
    return
  }

  const playerElement = player.el()

  if (!(playerElement instanceof HTMLElement)) {
    return
  }

  const controlBarElement = playerElement.querySelector<HTMLElement>('.vjs-control-bar')

  if (!controlBarElement) {
    return
  }

  const existingSpacer = controlBarElement.querySelector('.arachnea-videojs-div-control')

  if (!existingSpacer) {
    const spacer = document.createElement('div')
    spacer.className = 'arachnea-videojs-div-control'
    const durationElement = controlBarElement.querySelector('.vjs-duration')

    if (durationElement) {
      controlBarElement.insertBefore(spacer, durationElement.nextSibling)
    } else {
      controlBarElement.append(spacer)
    }
  }

  // Handle previous video control
  let prevButton = getPrevVideoControlElement(player)
  let createdPreviousControl = false

  if (!options.showPrevVideoControl) {
    prevButton?.remove()
  } else {
    if (!prevButton) {
      const prevBtn = document.createElement('button')
      prevBtn.type = 'button'
      prevBtn.className = 'vjs-control vjs-button vjs-prev-video-control'
      prevBtn.setAttribute('aria-label', t('entry.previousContent'))
      prevBtn.addEventListener('click', options.onPrevVideo)

      const qualityButton = controlBarElement.querySelector(
        '.vjs-quality-menu-wrapper, .vjs-quality-menu-button',
      )

      if (qualityButton) {
        controlBarElement.insertBefore(prevBtn, qualityButton)
      } else {
        const fullscreenButton = controlBarElement.querySelector('.vjs-fullscreen-control')

        if (fullscreenButton) {
          controlBarElement.insertBefore(prevBtn, fullscreenButton)
        } else {
          controlBarElement.append(prevBtn)
        }
      }
      prevButton = prevBtn
      createdPreviousControl = true
    }

    syncPrevNextVideoControlState(
      prevButton,
      !options.hasPreviousVideo,
      'entry.previousContent',
      options.previousVideoTitle,
    )
  }

  // Handle next video control
  let nextButton = getNextVideoControlElement(player)
  let createdNextControl = false

  if (!options.showNextVideoControl) {
    nextButton?.remove()
  } else {
    if (!nextButton) {
      const nextBtn = document.createElement('button')
      nextBtn.type = 'button'
      nextBtn.className = 'vjs-control vjs-button vjs-next-video-control'
      nextBtn.setAttribute('aria-label', t('entry.nextContent'))
      nextBtn.addEventListener('click', options.onNextVideo)

      const prevBtnForInsert = controlBarElement.querySelector('.vjs-prev-video-control')

      if (prevBtnForInsert && prevBtnForInsert.nextElementSibling) {
        controlBarElement.insertBefore(nextBtn, prevBtnForInsert.nextElementSibling)
      } else {
        const qualityButton = controlBarElement.querySelector(
          '.vjs-quality-menu-wrapper, .vjs-quality-menu-button',
        )

        if (qualityButton) {
          controlBarElement.insertBefore(nextBtn, qualityButton)
        } else {
          const fullscreenButton = controlBarElement.querySelector('.vjs-fullscreen-control')

          if (fullscreenButton) {
            controlBarElement.insertBefore(nextBtn, fullscreenButton)
          } else {
            controlBarElement.append(nextBtn)
          }
        }
      }
      nextButton = nextBtn
      createdNextControl = true
    }

    syncPrevNextVideoControlState(
      nextButton,
      !options.hasNextVideo,
      'entry.nextContent',
      options.nextVideoTitle,
    )
  }

  if (import.meta.env.DEV && (createdPreviousControl || createdNextControl)) {
    console.debug('[Video.js] Initialized episode navigation controls', {
      hasPreviousVideo: options.hasPreviousVideo,
      hasNextVideo: options.hasNextVideo,
      createdPreviousControl,
      createdNextControl,
    })
  }
}

/**
 * Repositions Video.js popup menus whose width depends on their entries.
 *
 * @param player Video.js player currently bound to the renderer.
 */
export function installAdaptiveControlBarMenuPositioning(player: VideoJsPlayer) {
  const playerElement = player.el()

  if (!(playerElement instanceof HTMLElement)) {
    return
  }

  let animationFrameId: number | null = null

  const updateOpenMenus = () => {
    animationFrameId = null

    getPositionableControlBarMenuButtons(playerElement).forEach((menuButtonElement) => {
      positionControlBarMenu(playerElement, menuButtonElement)
    })
  }

  const scheduleMenuUpdate = () => {
    if (animationFrameId !== null) {
      window.cancelAnimationFrame(animationFrameId)
    }

    animationFrameId = window.requestAnimationFrame(updateOpenMenus)
  }

  const resizeObserver = typeof ResizeObserver !== 'undefined'
    ? new ResizeObserver(scheduleMenuUpdate)
    : null

  resizeObserver?.observe(playerElement)
  playerElement.addEventListener('click', scheduleMenuUpdate, true)
  playerElement.addEventListener('keydown', scheduleMenuUpdate, true)
  window.addEventListener('resize', scheduleMenuUpdate)
  player.on('fullscreenchange', scheduleMenuUpdate)

  player.on('dispose', () => {
    if (animationFrameId !== null) {
      window.cancelAnimationFrame(animationFrameId)
      animationFrameId = null
    }

    resizeObserver?.disconnect()
    playerElement.removeEventListener('click', scheduleMenuUpdate, true)
    playerElement.removeEventListener('keydown', scheduleMenuUpdate, true)
    window.removeEventListener('resize', scheduleMenuUpdate)
    player.off('fullscreenchange', scheduleMenuUpdate)
  })
}

/**
 * Makes the timer toggle between elapsed and remaining time when activated.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param options Configuration and callback used to persist state changes.
 */
export function installTimerToggle(player: VideoJsPlayer, options: InstallTimerToggleOptions) {
  if (!options.controls) {
    return
  }

  const playerElement = player.el()

  if (!(playerElement instanceof HTMLElement)) {
    return
  }

  const currentTimeElement = playerElement.querySelector<HTMLElement>('.vjs-current-time')
  const remainingTimeElement = playerElement.querySelector<HTMLElement>('.vjs-remaining-time')

  if (!currentTimeElement || !remainingTimeElement) {
    return
  }

  playerElement.classList.remove(REMAINING_TIME_CLASS)

  const toggleTimerDisplay = () => {
    if (isLiveStream(player) || !isDurationAvailable(player)) {
      return
    }

    playerElement.classList.toggle(REMAINING_TIME_CLASS)
    syncTimerToggleState(playerElement, currentTimeElement, remainingTimeElement)
    options.onStateChange()
  }

  const handleTimerKeydown = (event: KeyboardEvent) => {
    if (event.key !== 'Enter' && event.key !== ' ') {
      return
    }

    event.preventDefault()
    toggleTimerDisplay()
  }

  for (const timerElement of [currentTimeElement, remainingTimeElement]) {
    timerElement.addEventListener('click', toggleTimerDisplay)
    timerElement.addEventListener('keydown', handleTimerKeydown)
  }

  syncTimerToggleState(playerElement, currentTimeElement, remainingTimeElement)
}

/**
 * Makes the progress bar seek to the clicked position.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param options Control-bar configuration for the current renderer.
 */
export function installSeekOnClick(player: VideoJsPlayer, options: InstallSeekOnClickOptions) {
  if (!options.controls) {
    return
  }

  const playerElement = player.el()

  if (!(playerElement instanceof HTMLElement)) {
    return
  }

  const progressControlElement = playerElement.querySelector<HTMLElement>('.vjs-progress-control')

  if (!progressControlElement) {
    return
  }

  const handleSeekOnClick = (event: MouseEvent) => {
    if (!isDurationAvailable(player) && !isLiveStream(player)) {
      return
    }

    const progressControlRect = progressControlElement.getBoundingClientRect()
    const clickX = event.clientX - progressControlRect.left
    const progressControlWidth = progressControlRect.width

    if (progressControlWidth <= 0) {
      return
    }

    if (!isLiveStream(player)) {
      return
    }

    event.stopPropagation()
    event.preventDefault()

    const clickRatio = clickX / progressControlWidth
    const seekableRange = getLiveSeekableRange(player)

    if (!seekableRange) {
      return
    }

    const targetTime =
      seekableRange.start + (clickRatio * (seekableRange.end - seekableRange.start))
    seekInLiveStream(player, targetTime)
  }

  progressControlElement.addEventListener('click', handleSeekOnClick, true)
}
