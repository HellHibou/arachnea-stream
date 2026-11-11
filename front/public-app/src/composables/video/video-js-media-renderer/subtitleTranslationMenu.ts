import videojs from 'video.js'

import type { VideoJsPlayer } from './types'
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
  /** Handles a click or keyboard activation. */
  handleClick?: () => void
  /** DOM element accessor exposed by Video.js components. */
  el?: () => HTMLElement
}

/** Minimal Video.js menu button handle used by the installer. */
interface SubtitleTranslationMenuButton {
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
 * Installs manual translation entries in the native Video.js subtitle menu.
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
  const createTranslationItem = (target: SubtitleTranslationMenuTarget) => {
    const item = new MenuItem(player, {
      label: target.label,
      selectable: false,
      multiSelectable: false,
    })
    item.handleClick = () => {
      button.unpressButton?.()
      button.focus?.()
      void source.requestManual(target.language)
    }
    item.el?.()?.classList.add('vjs-subtitle-translation-item')
    return item
  }

  button.createItems = function createItemsWithTranslation() {
    const items = originalCreateItems?.call(button) ?? []
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
