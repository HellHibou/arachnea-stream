import {
  EPISODE_AUTOPLAY_CONTROL_ACTIVE_CLASS,
  EPISODE_AUTOPLAY_CONTROL_CLASS,
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

interface InstallTimerToggleOptions {
  controls: boolean
  onStateChange: () => void
}

interface InstallSeekOnClickOptions {
  controls: boolean
}

const CONTROL_BAR_MENU_EDGE_PADDING_PX = 8

interface SyncEpisodeAutoplayToggleControlOptions {
  controls: boolean
  showEpisodeAutoplayToggle: boolean
  isEpisodeAutoplayEnabled: boolean
  onEpisodeAutoplayToggle: () => void
}

function clampMenuLeftOffset(value: number, min: number, max: number): number {
  if (max < min) {
    return min
  }

  return Math.min(Math.max(value, min), max)
}

function getEpisodeAutoplayToggleElement(player: VideoJsPlayer): HTMLButtonElement | null {
  return player.el()?.querySelector<HTMLButtonElement>(`.${EPISODE_AUTOPLAY_CONTROL_CLASS}`) ?? null
}

function shouldPositionControlBarMenu(menuButtonElement: HTMLElement): boolean {
  return (
    !menuButtonElement.classList.contains('vjs-quality-menu-button') &&
    !menuButtonElement.classList.contains('vjs-quality-menu-wrapper') &&
    !menuButtonElement.closest('.vjs-quality-menu-wrapper')
  )
}

function getPositionableControlBarMenuButtons(playerElement: HTMLElement): HTMLElement[] {
  const menuButtonElements = playerElement.querySelectorAll<HTMLElement>('.vjs-menu-button-popup')
  return Array.from(menuButtonElements).filter(shouldPositionControlBarMenu)
}

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

function syncEpisodeAutoplayToggleState(button: HTMLButtonElement, isEpisodeAutoplayEnabled: boolean) {
  const nextTitle = isEpisodeAutoplayEnabled ? t('player.disableAutoplay') : t('player.enableAutoplay')

  button.classList.toggle(EPISODE_AUTOPLAY_CONTROL_ACTIVE_CLASS, isEpisodeAutoplayEnabled, )
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
    button.textContent = 'Auto'
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
