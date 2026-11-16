import { t } from '@/i18n'
import type { VideoJsPlayer } from './types'
import type { VariantController } from './variantController'
import { positionControlBarMenu } from './controls'
import { getSelectedQualityLabel } from './quality'

/**
 * Installs in-player language and fixed-quality choices, including active manifest options.
 * @param player Ready Video.js instance.
 * @param variants Shared variant selection controller.
 * @returns Menu refresh and cleanup operations.
 */
export function installVariantMenus(player: VideoJsPlayer, variants: VariantController) {
  const bar = player.el()?.querySelector('.vjs-control-bar')
  const cleanups: Array<() => void> = []
  const refreshers: Array<() => void> = []
  for (const dimension of ['lang', 'quality'] as const) {
    /** Resolves the native component without matching the replacement's copied CSS classes. */
    const getNativeControl = (): HTMLElement | null => dimension === 'quality'
      ? (player.getChild('controlBar')?.getChild('QualityMenuButton')?.el() as HTMLElement | undefined) ?? null
      : bar?.querySelector<HTMLElement>('.vjs-audio-button') ?? null
    const wrapper = document.createElement('div')
    wrapper.className = 'vjs-menu-button vjs-menu-button-popup vjs-control vjs-button vjs-variant-menu'
    if (dimension === 'quality') wrapper.classList.add('vjs-quality-menu-button')
    const button = document.createElement('button')
    button.type = 'button'
    button.className = 'vjs-menu-button vjs-button'
    button.setAttribute('aria-haspopup', 'menu')
    button.setAttribute('aria-expanded', 'false')
    let icon = document.createElement('span')
    icon.className = `vjs-icon-placeholder ${dimension === 'quality' ? 'vjs-icon-cog' : 'vjs-icon-audio'}`
    icon.setAttribute('aria-hidden', 'true')
    const accessibleLabel = document.createElement('span')
    accessibleLabel.className = 'vjs-control-text'
    button.append(icon, accessibleLabel)
    const qualityValue = dimension === 'quality' ? document.createElement('span') : null
    if (qualityValue) {
      qualityValue.className = 'vjs-variant-quality-value'
      qualityValue.setAttribute('aria-hidden', 'true')
      button.append(qualityValue)
    }
    const menu = document.createElement('div')
    menu.className = 'vjs-menu'
    const list = document.createElement('ul')
    list.className = 'vjs-menu-content'
    list.setAttribute('role', 'menu')
    menu.append(list)
    wrapper.append(button, menu)
    const nativeControl = getNativeControl()
    bar?.insertBefore(wrapper, nativeControl ?? bar.querySelector('.vjs-fullscreen-control'))

    /** Closes the menu while preserving standard control-bar focus behavior. */
    const close = () => {
      menu.classList.remove('vjs-lock-showing')
      button.setAttribute('aria-expanded', 'false')
    }
    /**
     * Builds one keyboard accessible menu choice.
     * @param label Visible option label.
     * @param selected Whether this choice is active.
     * @param activate Action applied before closing the menu.
     */
    const addChoice = (label: string, selected: boolean, activate: () => void) => {
      const item = document.createElement('li')
      item.className = `vjs-menu-item${selected ? ' vjs-selected' : ''}`
      item.textContent = label
      item.tabIndex = -1
      item.setAttribute('role', 'menuitemradio')
      item.setAttribute('aria-checked', String(selected))
      const choose = () => { activate(); refresh(); close(); button.focus() }
      item.addEventListener('click', choose)
      item.addEventListener('keydown', (event) => {
        if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); choose() }
      })
      list.append(item)
    }
    /** Derives fixed qualities for the active language, or the collection's language choices. */
    const availableValues = () => {
      const activeLanguage = variants.variants.value[variants.activeIndex.value]?.lang
      return [...new Set(variants.variants.value
        .filter((variant) => dimension === 'lang' || variant.lang === activeLanguage)
        .map((variant) => variant[dimension]))]
    }
    /** Reconciles source dimensions and the active manifest's native track/quality menu. */
    const refresh = () => {
      const values = availableValues()
      const visible = values.length > 1 && values.some((value) => value !== null)
      wrapper.style.display = visible ? '' : 'none'
      const native = getNativeControl()
      if (native) {
        native.style.display = visible ? 'none' : ''
        if (dimension === 'quality') {
          const nativeButton = native.querySelector<HTMLButtonElement>('button')
          const nativeIcon = nativeButton?.querySelector<HTMLElement>('.vjs-icon-placeholder')
          if (nativeButton) button.className = nativeButton.className
          if (nativeIcon) {
            const replacement = nativeIcon.cloneNode(true) as HTMLElement
            icon.replaceWith(replacement)
            icon = replacement
          }
        }
        if (dimension === 'quality' && native.parentElement === bar && wrapper.nextElementSibling !== native) {
          bar?.insertBefore(wrapper, native)
        }
      }
      const title = t(dimension === 'quality' ? 'player.streamQuality' : 'player.streamLanguage')
      const qualityLabel = variants.variants.value[variants.activeIndex.value]?.quality ?? getSelectedQualityLabel(player) ?? t('player.streamAutomatic')
      if (qualityValue) qualityValue.textContent = qualityLabel
      button.setAttribute('aria-label', qualityValue ? `${title}: ${qualityLabel}` : title)
      button.title = title
      accessibleLabel.textContent = title
      if (!visible) close()
    }
    /** Rebuilds options on open so asynchronously discovered manifest options are current. */
    const open = () => {
      refresh()
      list.replaceChildren()
      const active = variants.variants.value[variants.activeIndex.value]
      const values = availableValues()
      for (const value of values) {
        const available = variants.variants.value.some((variant, index) => variant[dimension] === value && variants.isAvailable(index))
        if (!available) continue
        const label = value ?? t(dimension === 'quality' ? 'player.streamAutomatic' : 'player.undefined')
        addChoice(label, active?.[dimension] === value, () => variants.select(dimension, value))
      }
      for (const nativeItem of Array.from(getNativeControl()?.querySelectorAll<HTMLElement>('.vjs-menu-item') ?? [])) {
        const label = nativeItem.textContent?.trim()
        if (label) addChoice(label, nativeItem.classList.contains('vjs-selected'), () => nativeItem.click())
      }
      menu.classList.add('vjs-lock-showing')
      button.setAttribute('aria-expanded', 'true')
      if (dimension !== 'quality') positionControlBarMenu(player.el() as HTMLElement, wrapper)
      list.querySelector<HTMLElement>('.vjs-selected')?.focus()
    }
    button.addEventListener('click', () => menu.classList.contains('vjs-lock-showing') ? close() : open())
    wrapper.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') { event.preventDefault(); close(); button.focus() }
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
      event.preventDefault()
      if (!menu.classList.contains('vjs-lock-showing')) open()
      const items = Array.from(list.querySelectorAll<HTMLElement>('.vjs-menu-item'))
      const current = items.indexOf(document.activeElement as HTMLElement)
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (current + (event.key === 'ArrowUp' ? -1 : 1) + items.length) % items.length
      items[next]?.focus()
    })
    const outside = (event: Event) => { if (event.target instanceof Node && !wrapper.contains(event.target)) close() }
    document.addEventListener('pointerdown', outside)
    player.on(['loadstart', 'loadedmetadata'], refresh)
    refreshers.push(refresh)
    cleanups.push(() => {
      player.off(['loadstart', 'loadedmetadata'], refresh)
      document.removeEventListener('pointerdown', outside)
      const native = getNativeControl()
      if (native) native.style.display = ''
      wrapper.remove()
    })
  }
  return {
    refresh: () => refreshers.forEach((refresh) => refresh()),
    dispose: () => cleanups.forEach((cleanup) => cleanup()),
  }
}
