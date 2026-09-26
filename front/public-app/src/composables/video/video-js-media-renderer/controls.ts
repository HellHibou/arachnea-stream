import {
  EPISODE_AUTOPLAY_CONTROL_ACTIVE_CLASS,
  EPISODE_AUTOPLAY_CONTROL_CLASS,
  EPISODE_AUTOPLAY_MENU_CLASS,
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

/** Entry rendered as a checkbox item inside the episode autoplay menu. */
export interface AutoplayMenuEntry {
  /** Stable entry identifier, also used to restore focus after a rebuild. */
  key: string
  /** Translation key of the entry label. */
  labelKey: string
  /** Whether the entry is currently enabled. */
  isEnabled: boolean
  /** Callback invoked when the entry is activated. */
  onToggle: () => void
}

/** Options for syncing the episode autoplay menu control. */
interface SyncEpisodeAutoplayToggleControlOptions {
  /** Whether player controls are enabled. */
  controls: boolean
  /** Whether the episode autoplay menu should be shown. */
  showEpisodeAutoplayToggle: boolean
  /** Whether episode autoplay is currently enabled. */
  isEpisodeAutoplayEnabled: boolean
  /** Entries rendered as checkbox items inside the menu. */
  menuEntries: AutoplayMenuEntry[]
  /** Callback invoked when the menu opens so stale entries are refreshed. */
  onMenuOpen?: () => void
}

/** Live handle registered for each mounted episode autoplay menu. */
interface EpisodeAutoplayMenuHandle {
  /** Options captured by the latest sync. */
  options: SyncEpisodeAutoplayToggleControlOptions
  /** Closes the menu and releases its outside-click listener. */
  close: () => void
}

/** Registry of mounted episode autoplay menus keyed by their wrapper element. */
const episodeAutoplayMenus = new WeakMap<HTMLElement, EpisodeAutoplayMenuHandle>()

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
 * Gets the episode autoplay menu wrapper element from the player.
 *
 * @param player - Video.js player instance.
 * @returns Episode autoplay menu wrapper element or null.
 */
function getEpisodeAutoplayMenuElement(player: VideoJsPlayer): HTMLElement | null {
  return player.el()?.querySelector<HTMLElement>(`.${EPISODE_AUTOPLAY_MENU_CLASS}`) ?? null
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
 * Activates a menu entry from its item element using the latest synced options.
 *
 * The entry is resolved again at activation time so the callback always comes
 * from the latest sync instead of the one captured when the item was built.
 *
 * @param item - Menu item element carrying the entry key.
 */
function activateEpisodeAutoplayMenuEntry(item: HTMLElement) {
  const entryKey = item.dataset.autoplayMenuEntry
  const wrapper = item.closest<HTMLElement>(`.${EPISODE_AUTOPLAY_MENU_CLASS}`)

  if (!entryKey || !wrapper) {
    return
  }

  const entry = episodeAutoplayMenus.get(wrapper)?.options.menuEntries
    .find((candidate) => candidate.key === entryKey)

  entry?.onToggle()
}

/**
 * Rebuilds the menu content when the entry list shape changed.
 *
 * @param contentElement - Menu content element holding the items.
 * @param entries - Entries to render.
 */
function buildEpisodeAutoplayMenuItems(contentElement: HTMLElement, entries: AutoplayMenuEntry[]) {
  const focusedKey = contentElement.querySelector<HTMLElement>(':focus')?.dataset.autoplayMenuEntry ?? null

  contentElement.replaceChildren()

  for (const entry of entries) {
    const item = document.createElement('li')
    item.className = 'vjs-menu-item'
    item.dataset.autoplayMenuEntry = entry.key
    item.setAttribute('role', 'menuitemcheckbox')
    item.setAttribute('tabindex', '0')
    item.addEventListener('click', () => activateEpisodeAutoplayMenuEntry(item))
    item.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter' && event.key !== ' ') {
        return
      }

      event.preventDefault()
      activateEpisodeAutoplayMenuEntry(item)
    })
    contentElement.append(item)
  }

  if (focusedKey) {
    contentElement
      .querySelector<HTMLElement>(`:scope > [data-autoplay-menu-entry="${focusedKey}"]`)
      ?.focus()
  }
}

/**
 * Synchronizes the checkbox items of the episode autoplay menu.
 *
 * Labels and states are refreshed on every sync so they follow language and
 * parameter changes. The items are only rebuilt when the entry list itself
 * changed, so activating an item never detaches it mid-click.
 *
 * @param menuElement - Menu element holding the content list.
 * @param options - Latest sync options carrying the entries to render.
 */
function syncEpisodeAutoplayMenuEntries(
  menuElement: HTMLElement,
  options: SyncEpisodeAutoplayToggleControlOptions,
) {
  const contentElement = menuElement.querySelector<HTMLElement>('.vjs-menu-content')

  if (!contentElement) {
    return
  }

  const entries = options.menuEntries
  const items = Array.from(contentElement.querySelectorAll<HTMLElement>(':scope > .vjs-menu-item'))
  const hasSameShape =
    items.length === entries.length &&
    items.every((item, index) => item.dataset.autoplayMenuEntry === entries[index]?.key)

  if (!hasSameShape) {
    buildEpisodeAutoplayMenuItems(contentElement, entries)
  }

  const syncedItems = Array.from(contentElement.querySelectorAll<HTMLElement>(':scope > .vjs-menu-item'))

  for (const [index, item] of syncedItems.entries()) {
    const entry = entries[index]

    if (!entry) {
      continue
    }

    item.textContent = t(entry.labelKey)
    item.setAttribute('aria-checked', String(entry.isEnabled))
    item.classList.toggle('vjs-selected', entry.isEnabled)
  }
}

/**
 * Synchronizes the visual state of the episode autoplay menu control.
 *
 * @param wrapper - Episode autoplay menu wrapper element.
 * @param options - Latest sync options.
 */
function syncEpisodeAutoplayToggleState(
  wrapper: HTMLElement,
  options: SyncEpisodeAutoplayToggleControlOptions,
) {
  const button = wrapper.querySelector<HTMLButtonElement>(`.${EPISODE_AUTOPLAY_CONTROL_CLASS}`)
  const menuElement = wrapper.querySelector<HTMLElement>(':scope > .vjs-menu')

  if (!button || !menuElement) {
    return
  }

  const menuLabel = t('player.autoplayMenu.label')

  button.classList.toggle(EPISODE_AUTOPLAY_CONTROL_ACTIVE_CLASS, options.isEpisodeAutoplayEnabled)
  button.setAttribute('aria-label', menuLabel)
  button.setAttribute('title', menuLabel)
  button.setAttribute('aria-expanded', String(menuElement.classList.contains('vjs-lock-showing')))

  syncEpisodeAutoplayMenuEntries(menuElement, options)
}

/**
 * Creates the episode autoplay menu wrapper around its toggle button.
 *
 * The wrapper follows the Video.js popup pattern: a `div` holding the toggle
 * button and a `.vjs-menu` sibling. The menu stays open when an entry is
 * activated and closes on outside clicks, on Escape, and when the controls go
 * inactive.
 *
 * @param player - Video.js player instance.
 * @returns Wrapper element together with its close helper.
 */
function createEpisodeAutoplayMenu(player: VideoJsPlayer): {
  wrapper: HTMLElement
  close: () => void
} {
  const playerElement = player.el()
  const wrapper = document.createElement('div')
  wrapper.className = `vjs-menu-button vjs-menu-button-popup ${EPISODE_AUTOPLAY_MENU_CLASS}`

  const button = document.createElement('button')
  button.type = 'button'
  button.className = `vjs-control vjs-button ${EPISODE_AUTOPLAY_CONTROL_CLASS}`
  button.setAttribute('aria-haspopup', 'true')
  button.setAttribute('aria-expanded', 'false')
  button.innerHTML =
    '<span class="vjs-episode-autoplay-track"><span class="vjs-episode-autoplay-thumb"><span class="vjs-episode-autoplay-icon"></span></span></span>'

  const menuElement = document.createElement('div')
  menuElement.className = 'vjs-menu'
  const contentElement = document.createElement('ul')
  contentElement.className = 'vjs-menu-content'
  menuElement.append(contentElement)
  wrapper.append(button, menuElement)

  const isOpen = () => menuElement.classList.contains('vjs-lock-showing')

  const close = () => {
    if (!isOpen()) {
      return
    }

    menuElement.classList.remove('vjs-lock-showing')
    button.setAttribute('aria-expanded', 'false')
    document.removeEventListener('click', handleOutsideClick)

    if (document.activeElement !== button && wrapper.contains(document.activeElement)) {
      button.focus()
    }
  }

  function handleOutsideClick(event: MouseEvent) {
    // The composed path is captured at dispatch start, so an item replaced by
    // a re-sync during this very click still counts as an inside click.
    if (!event.composedPath().includes(wrapper)) {
      close()
    }
  }

  const open = () => {
    menuElement.classList.add('vjs-lock-showing')
    button.setAttribute('aria-expanded', 'true')
    document.addEventListener('click', handleOutsideClick)
    // Refresh the entries first so a change made from the parameters panel
    // while the player was mounted never shows stale states.
    episodeAutoplayMenus.get(wrapper)?.options.onMenuOpen?.()

    if (playerElement instanceof HTMLElement) {
      positionControlBarMenu(playerElement, wrapper)
    }

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

  player.on('userinactive', close)
  player.on('dispose', () => {
    document.removeEventListener('click', handleOutsideClick)
  })

  return { wrapper, close }
}

/**
 * Mounts or updates the episode autoplay menu inside the Video.js control bar.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param options Reactive configuration and callbacks used by the control.
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

  let menuWrapper = getEpisodeAutoplayMenuElement(player)

  if (!options.showEpisodeAutoplayToggle) {
    if (menuWrapper) {
      episodeAutoplayMenus.get(menuWrapper)?.close()
      menuWrapper.remove()
    }

    return
  }

  if (!menuWrapper) {
    const created = createEpisodeAutoplayMenu(player)
    menuWrapper = created.wrapper
    episodeAutoplayMenus.set(menuWrapper, { options, close: created.close })

    const qualityButton = controlBarElement.querySelector(
      '.vjs-quality-menu-wrapper, .vjs-quality-menu-button',
    )
    const fullscreenButton = controlBarElement.querySelector('.vjs-fullscreen-control')
    const anchorButton = qualityButton ?? fullscreenButton

    if (anchorButton) {
      controlBarElement.insertBefore(menuWrapper, anchorButton)
    } else {
      controlBarElement.append(menuWrapper)
    }
  }

  const menuHandle = episodeAutoplayMenus.get(menuWrapper)

  if (menuHandle) {
    menuHandle.options = options
  }

  syncEpisodeAutoplayToggleState(menuWrapper, options)
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
  /** Whether the previous video control native title is replaced by a rich preview. */
  suppressPreviousVideoTitle: boolean
  /** Whether the next video control native title is replaced by a rich preview. */
  suppressNextVideoTitle: boolean
  /** Callback invoked when the previous video button is clicked. */
  onPrevVideo: () => void
  /** Callback invoked when the next video button is clicked. */
  onNextVideo: () => void
  /** Callback invoked when the previous video control preview state changes. */
  onPrevVideoPreviewChange: (isPreviewed: boolean) => void
  /** Callback invoked when the next video control preview state changes. */
  onNextVideoPreviewChange: (isPreviewed: boolean) => void
}

/**
 * Synchronizes the visual state of a prev/next video control button.
 *
 * @param button - Control button element.
 * @param isHidden - Whether the button should be hidden.
 * @param labelKey - Translation key for the aria-label.
 * @param videoTitle - Neighbouring media title exposed through the native tooltip.
 * @param suppressNativeTitle - Whether the native tooltip is replaced by a rich preview.
 */
function syncPrevNextVideoControlState(
  button: HTMLButtonElement | null,
  isHidden: boolean,
  labelKey: string,
  videoTitle: string | null,
  suppressNativeTitle: boolean,
) {
  if (!button) {
    return
  }
  button.disabled = isHidden
  button.classList.toggle('vjs-prev-video-control--hidden', isHidden && button.classList.contains('vjs-prev-video-control'))
  button.classList.toggle('vjs-next-video-control--hidden', isHidden && button.classList.contains('vjs-next-video-control'))
  button.setAttribute('aria-label', t(labelKey))

  if (suppressNativeTitle) {
    button.removeAttribute('title')
    return
  }

  button.title = videoTitle?.trim() || t(labelKey)
}

/**
 * Registers the preview lifecycle listeners used by the rich navigation preview.
 *
 * @param button - Control button element receiving the listeners.
 * @param onPreviewChange - Callback invoked when the control preview state changes.
 */
function registerPrevNextVideoPreviewListeners(
  button: HTMLButtonElement,
  onPreviewChange: (isPreviewed: boolean) => void,
) {
  button.addEventListener('pointerenter', () => onPreviewChange(true))
  button.addEventListener('pointerleave', () => onPreviewChange(false))
  button.addEventListener('focus', () => onPreviewChange(true))
  button.addEventListener('blur', () => onPreviewChange(false))
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
      registerPrevNextVideoPreviewListeners(prevBtn, options.onPrevVideoPreviewChange)

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
      options.suppressPreviousVideoTitle,
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
      registerPrevNextVideoPreviewListeners(nextBtn, options.onNextVideoPreviewChange)

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
      options.suppressNextVideoTitle,
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
