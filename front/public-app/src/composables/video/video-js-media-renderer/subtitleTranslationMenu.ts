import videojs from 'video.js'

import { t } from '@/i18n'
import { getBaseLanguageCode, getLanguageFlag, normalizeLanguageCode } from '@/services/languages'
import type { VideoJsPlayer } from './types'
import { createSubtitleOffsetControl, type SubtitleOffsetControl } from './subtitleOffset'
import type {
  SubtitleTranslationMenuTarget,
} from '@/composables/video/useSubtitleTranslationController'

/** Source of manual translation entries rendered in the native subtitle menu. */
export interface SubtitleTranslationMenuSource {
  /** Returns the currently available manual translation targets. */
  getManualTargets: () => SubtitleTranslationMenuTarget[]
  /** Starts one manual translation. */
  requestManual: (language: string) => void | Promise<void>
}

/** Minimal Video.js menu item handle used by the installer. */
interface SubtitleTranslationMenuItem {
  /** Text track represented by a native menu item. */
  track?: { language?: string }
  /** Handles a click or keyboard activation. */
  handleClick?: () => void
  /** DOM element accessor exposed by Video.js components. */
  el?: () => HTMLElement
}

/** Minimal Video.js menu button handle used by the installer. */
interface SubtitleTranslationMenuButton {
  /** DOM element containing the button and its popup. */
  el?: () => Element
  /** Builds the native subtitle menu items. */
  createItems?: () => SubtitleTranslationMenuItem[]
  /** Rebuilds the menu from `createItems`. */
  update?: () => void
  /** Closes the popup menu. */
  unpressButton?: () => void
  /** Returns focus to the menu button. */
  focus?: () => void
}

/** Constructor returned by `videojs.getComponent('MenuItem')`. */
type MenuItemConstructor = new (
  player: VideoJsPlayer,
  options: Record<string, unknown>,
) => SubtitleTranslationMenuItem

/** Installed menu controls returned to the renderer. */
export interface SubtitleTranslationMenuInstallation {
  /** Refreshes entries after tracks, catalog or translation status changes. */
  refresh: () => void
  /** Restores the original native menu. */
  dispose: () => void
}

/** Child names used by Video.js releases for the subtitle menu button. */
const SUBTITLE_MENU_BUTTON_NAMES = [
  'SubsCapsButton',
  'subsCapsButton',
  'SubtitlesButton',
  'subtitlesButton',
  'CaptionsButton',
  'captionsButton',
]

/**
 * Adds language flags to the native audio track menu as its items are rebuilt.
 * @param player Player owning the audio menu.
 */
export function installAudioMenuFlags(player: VideoJsPlayer): void {
  const controlBar = player.getChild('controlBar')
  const button = (controlBar?.getChild('AudioTrackButton') ??
    controlBar?.getChild('audioTrackButton')) as SubtitleTranslationMenuButton | null
  if (!button?.createItems) return
  const originalCreateItems = button.createItems
  button.createItems = function createItemsWithFlags() {
    const items = originalCreateItems.call(button)
    for (const item of items) addSubtitleMenuIcon(item, item.track?.language)
    return items
  }
  button.update?.()
  player.on('dispose', () => { button.createItems = originalCreateItems })
}

/**
 * Installs a temporary subtitle delay and manual translation entries in the native menu.
 *
 * Existing Off/Captions/subtitle entries are preserved. Translation entries are
 * rebuilt from the controller every time Video.js refreshes the menu.
 *
 * @param player Player owning the control bar.
 * @param source Controller exposing manual targets and requests.
 * @returns Refresh and cleanup operations, or `null` when no menu exists.
 */
export function installSubtitleTranslationMenu(
  player: VideoJsPlayer,
  source: SubtitleTranslationMenuSource,
): SubtitleTranslationMenuInstallation | null {
  const button = findSubtitleMenuButton(player)
  if (!button) {
    return null
  }

  const MenuItem = resolveMenuItemConstructor()
  if (!MenuItem) {
    return null
  }

  const originalCreateItems = button.createItems
  const offset = createSubtitleOffsetControl(player)
  const closeOnOutsideClick = (event: MouseEvent) => {
    const element = button.el?.()
    if (element && !event.composedPath().includes(element)) button.unpressButton?.()
  }
  document.addEventListener('click', closeOnOutsideClick, true)
  const OffTextTrackMenuItem = videojs.getComponent('OffTextTrackMenuItem')
  const createTranslationItem = (target: SubtitleTranslationMenuTarget) => {
    const item = new MenuItem(player, {
      label: target.label,
      selectable: false,
      multiSelectable: false,
    })
    item.handleClick = () => {
      if (target.pending) return
      button.unpressButton?.()
      button.focus?.()
      void source.requestManual(target.language)
    }
    item.el?.()?.classList.add('vjs-subtitle-translation-item')
    if (target.pending) item.el?.()?.setAttribute('aria-disabled', 'true')
    addSubtitleMenuIcon(item, target.language)
    return item
  }

  button.createItems = function createItemsWithTranslation() {
    const items = originalCreateItems?.call(button) ?? []
    for (const item of items) {
      const element = item.el?.()
      const icon = element?.classList.contains('vjs-texttrack-settings')
        ? 'mdi-cog'
        : OffTextTrackMenuItem && item instanceof OffTextTrackMenuItem
          ? 'mdi-eye-off-outline'
          : null
      addSubtitleMenuIcon(item, item.track?.language, icon)
    }
    const offsetItem = createSubtitleOffsetItem(player, MenuItem, offset)
    items.unshift(offsetItem)
    // Video.js otherwise closes the menu when focus enters a child input.
    for (const item of items) {
      item.el?.()?.addEventListener('blur', (event) => {
        if (event.relatedTarget instanceof Node && offsetItem.el?.()?.contains(event.relatedTarget)) {
          event.stopImmediatePropagation()
        }
      }, true)
    }
    return [
      ...items,
      ...source.getManualTargets().map(createTranslationItem),
    ]
  }

  const refresh = () => {
    if ((player as unknown as { isDisposed?: () => boolean }).isDisposed?.()) {
      return
    }
    button.update?.()
  }

  refresh()

  return {
    refresh,
    dispose: () => {
      document.removeEventListener('click', closeOnOutsideClick, true)
      offset.dispose()
      if (button.createItems === originalCreateItems) {
        return
      }
      if (originalCreateItems) {
        button.createItems = originalCreateItems
      } else {
        delete button.createItems
      }
      refresh()
    },
  }
}

/**
 * Builds an editable seconds field without invoking menu selection or player hotkeys.
 * @param player Player owning the menu item.
 * @param MenuItem Native Video.js menu item constructor.
 * @param offset In-memory subtitle timing control.
 * @returns Menu item containing an accessible numeric input.
 */
function createSubtitleOffsetItem(
  player: VideoJsPlayer,
  MenuItem: MenuItemConstructor,
  offset: SubtitleOffsetControl,
): SubtitleTranslationMenuItem {
  const item = new MenuItem(player, { label: '', selectable: false })
  item.handleClick = () => {}
  const element = item.el?.()
  const text = element?.querySelector('.vjs-menu-item-text')
  if (!element || !text) return item

  element.classList.add('vjs-subtitle-offset-item')
  element.setAttribute('role', 'presentation')
  const label = document.createElement('label')
  label.textContent = t('player.subtitles.offset')
  const input = document.createElement('input')
  input.type = 'number'
  input.step = '0.1'
  input.value = String(offset.getSeconds())
  input.title = t('player.subtitles.offsetHint')
  input.addEventListener('input', () => {
    if (input.value !== '' && input.validity.valid) offset.setSeconds(input.valueAsNumber)
  })
  input.addEventListener('blur', () => {
    input.value = String(offset.getSeconds())
  })
  input.addEventListener('keydown', (event) => {
    if (event.key !== 'Escape' && event.key !== 'Tab') event.stopPropagation()
  })
  label.addEventListener('click', (event) => event.stopPropagation())
  label.addEventListener('touchstart', (event) => event.stopPropagation())
  element.addEventListener('click', (event) => event.stopImmediatePropagation())
  element.addEventListener('focus', () => input.focus())
  label.appendChild(input)
  text.appendChild(label)
  addSubtitleMenuIcon(item, undefined, 'mdi-timer-outline')
  return item
}

/**
 * Adds a decorative icon slot to a subtitle menu item.
 * @param item Menu item to decorate.
 * @param language Language code for a track or translation target.
 * @param icon Material Design icon class for non-language actions.
 */
function addSubtitleMenuIcon(
  item: SubtitleTranslationMenuItem,
  language?: string,
  icon: string | null = null,
) {
  const element = item.el?.()
  const label = element?.querySelector('.vjs-menu-item-text')
  if (!element || !label) return

  const slot = document.createElement('span')
  slot.className = 'vjs-subtitle-menu-icon'
  slot.setAttribute('aria-hidden', 'true')
  if (icon) {
    slot.classList.add('mdi', icon)
  } else {
    slot.textContent = getLanguageFlag(getBaseLanguageCode(normalizeLanguageCode(language))) ?? ''
  }
  element.insertBefore(slot, label)
}

/**
 * Finds the native subtitle menu button in the control bar.
 *
 * @param player Active player.
 * @returns Menu button handle, or `null`.
 */
function findSubtitleMenuButton(
  player: VideoJsPlayer,
): SubtitleTranslationMenuButton | null {
  const controlBar = player.getChild('controlBar')

  for (const name of SUBTITLE_MENU_BUTTON_NAMES) {
    const button = controlBar?.getChild(name) as SubtitleTranslationMenuButton | null
    if (button) {
      return button
    }
  }

  return null
}

/**
 * Resolves the Video.js `MenuItem` constructor.
 *
 * @returns MenuItem constructor, or `null` when the component is unavailable.
 */
function resolveMenuItemConstructor(): MenuItemConstructor | null {
  const components = videojs as unknown as {
    getComponent?: (name: string) => MenuItemConstructor | null | undefined
  }
  return components.getComponent?.('MenuItem') ?? null
}
