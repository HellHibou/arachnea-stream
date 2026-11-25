import { reportFrontendErrorNotification } from '@/composables/useErrorNotifications'
import { t } from '@/i18n'
import { formatLanguageLabel, getBaseLanguageCode, getLanguageFlag, normalizeLanguageCode } from '@/services/languages'
import type { ResolvedVideoMediaSource } from '@/services/players'
import type { ResolvedPlayerSubtitle } from '@/types/entry'
import { resolveChapterLabel } from './chapterMenu'
import { positionControlBarMenu } from './controls'
import { getDashSubtitleDownloads } from './dash/subtitleDownloads'
import { captureVideoFrame, cleanFileName, downloadFile, ProtectedFrameError } from './downloads'
import { discoverNativeHlsSubtitles, hlsSubtitleDownloads, readHlsSubtitleSources } from './hlsSubtitleDownloads'
import { createStoryboardArchive } from './storyboardExport'
import { formatVttTime, type SubtitleDownload } from './subtitleExport'
import { getSubtitleFileLabel } from './subtitles'
import type { VideoJsPlayer } from './types'

/** Minimum time a fast download keeps its status visible in the player. */
const MIN_STATUS_VISIBLE_MS = 1_500

interface DownloadItem {
  icon: string
  iconClass?: string
  label: string
  run: (signal: AbortSignal) => Promise<void>
  failureKey: string
  statusKey?: string
  errorOperationKey?: string
  assembling?: boolean
  disabled?: boolean
}

/**
 * Builds the available actions from the current source collection and playback state.
 * @param source Active variant and shared media metadata.
 * @param title Displayed media title.
 * @param playerLabel Displayed label of the selected player, when applicable.
 * @param video Current Video.js tech element, if present.
 * @param protectedPlayback Whether the active stream or browser signals DRM.
 * @param subtitles Downloadable tracks discovered from the active player.
 * @param duration Video duration in seconds, used by the sprite storyboard fallback.
 */
function buildDownloadGroups(
  source: ResolvedVideoMediaSource,
  title: string | null | undefined,
  playerLabel: string | null | undefined,
  video: HTMLVideoElement | null,
  protectedPlayback: boolean,
  subtitles: SubtitleDownload[],
  duration: number,
): DownloadItem[][] {
  const baseName = cleanFileName(
    source.fileName?.replace(/\s*\.(?:mp4|m4v|webm|ogg|ogv|m3u8|mpd)$/i, ''),
    cleanFileName(title ?? source.title, 'video'),
  )
  const mp4s = (source.sources ?? [{ ...source, quality: null, lang: null }])
    .filter((variant) => variant.transport === 'file' && variant.mimeType === 'video/mp4')
  const videoItems = mp4s.map(({ src, lang, quality }, index): DownloadItem => {
    const details = [lang && formatLanguageLabel(lang), quality].filter(Boolean).join(' · ')
    const variantLabel = mp4s.length > 1 ? details || String(index + 1) : ''
    const suffix = [cleanFileName(playerLabel, ''), cleanFileName(variantLabel, '')].filter(Boolean).join(' - ')
    const filename = suffix ? `${baseName} - ${suffix}.mp4` : `${baseName}.mp4`
    return {
      icon: 'video-outline',
      label: ['MP4', playerLabel, details || variantLabel].filter(Boolean).join(' · '),
      statusKey: 'player.downloadMenu.downloadingVideo',
      run: () => downloadFile(src, filename, Boolean(suffix)),
      failureKey: 'player.downloadMenu.downloadFailed',
    }
  })
  const subtitleItems = subtitles.map(({ lang, label, link, extension, assemble }, index): DownloadItem => {
    const flag = getLanguageFlag(getBaseLanguageCode(normalizeLanguageCode(lang ?? null)))
    const trackLabel = label ?? getSubtitleFileLabel(link) ?? t('player.downloadMenu.subtitle', { index: index + 1 })
    const details = [flag, trackLabel].filter(Boolean).join(' ')
    return {
      icon: 'subtitles-outline',
      label: details,
      async run(signal) {
        const file = assemble ? await assemble(signal) : link
        signal.throwIfAborted()
        await downloadFile(file, `${baseName} - ${cleanFileName(lang, 'und')} - ${cleanFileName(trackLabel, String(index + 1))}.${extension}`)
      },
      assembling: Boolean(assemble),
      failureKey: 'player.downloadMenu.downloadFailed',
    }
  })
  const metadataItems: DownloadItem[] = []
  if (source.storyboardVttUrl || (source.storyboard && duration > 0)) {
    metadataItems.push({
      icon: 'image-multiple-outline',
      label: t('player.downloadMenu.storyboard'),
      statusKey: 'player.downloadMenu.downloadingStoryboard',
      errorOperationKey: 'errors.notifications.operations.storyboardExport',
      assembling: true,
      async run(signal) {
        const archive = await createStoryboardArchive(source, duration, signal)
        signal.throwIfAborted()
        await downloadFile(archive, `${baseName} - storyboard.zip`)
      },
      failureKey: 'player.downloadMenu.storyboardFailed',
    })
  }
  const chapters = source.chapters
  if (chapters?.length) {
    metadataItems.push({
      icon: 'bookmark-outline',
      iconClass: 'vjs-download-icon-chapters',
      label: t('player.downloadMenu.chapters'),
      errorOperationKey: 'errors.notifications.operations.chapterExport',
      async run(signal) {
        const cues = chapters
          .filter((chapter) => chapter.end > chapter.start)
          .map((chapter) =>
            `${formatVttTime(chapter.start)} --> ${formatVttTime(chapter.end)}\n${resolveChapterLabel(chapter)}`,
          )
        const vtt = new Blob(['WEBVTT\n\n', cues.join('\n\n'), '\n'], { type: 'text/vtt;charset=utf-8' })
        signal.throwIfAborted()
        await downloadFile(vtt, `${baseName} - chapters.vtt`)
      },
      failureKey: 'player.downloadMenu.chapterExportFailed',
    })
  }
  const screenshotItems: DownloadItem[] = protectedPlayback ? [] : [{
    icon: 'camera-outline',
    label: t('player.downloadMenu.screenshot'),
    disabled: !video || video.readyState < HTMLMediaElement.HAVE_CURRENT_DATA || !video.videoWidth,
    async run() {
      if (!video) return
      const frame = await captureVideoFrame(video)
      await downloadFile(frame, `${baseName}.png`)
    },
    failureKey: 'player.downloadMenu.screenshotFailed',
  }]
  return [videoItems, subtitleItems, metadataItems, screenshotItems].filter((group) => group.length > 0)
}

/**
 * Reads the complete sources of tracks injected into Video.js or its video element.
 * @param player Active Video.js player.
 * @param video Its current video element.
 * @param tracks Subtitle URLs provided with the source or generated by translation.
 */
function readInjectedSubtitles(player: VideoJsPlayer, video: HTMLVideoElement | null, tracks: ResolvedPlayerSubtitle[]): SubtitleDownload[] {
  const remote = player.remoteTextTrackEls?.() as unknown as ArrayLike<HTMLTrackElement> | undefined
  const elements = [...Array.from(remote ?? []), ...(video?.querySelectorAll('track') ?? [])]
  const injected = elements.flatMap((track): ResolvedPlayerSubtitle[] =>
    (track.kind === 'subtitles' || track.kind === 'captions') && track.src
      ? [{ link: track.src, lang: track.srclang || undefined, label: track.label || undefined }]
      : [],
  )
  return [...tracks, ...injected].flatMap((track): SubtitleDownload[] => {
    try {
      const url = new URL(track.link, document.baseURI)
      if (url.protocol !== 'blob:' && url.protocol !== 'http:' && url.protocol !== 'https:' && url.protocol !== location.protocol) return []
      return [{ ...track, link: url.href, extension: /\.(ttml|dfxp)$/i.test(url.pathname) ? 'ttml' : 'vtt' }]
    } catch {
      return []
    }
  })
}

/**
 * Creates a decorative icon using the font already loaded by the public app.
 * @param name Material Design Icons glyph name.
 * @param customClass Optional class replacing the icon font for an SVG asset.
 */
function createIcon(name: string, customClass?: string): HTMLSpanElement {
  const icon = document.createElement('span')
  icon.className = customClass ?? `mdi mdi-${name}`
  icon.setAttribute('aria-hidden', 'true')
  return icon
}

/**
 * Adds the download popup to the Video.js control bar for this player instance.
 * @param player Video.js player owning the control bar.
 * @param getSource Reader for the currently selected variant and its alternatives.
 * @param getTitle Reader for the displayed media title.
 * @param getTranslation Reader for the current translated subtitle.
 * @param getPlayerLabel Reader for the selected player label, when applicable.
 * @param setStatus Publishes or clears this menu's message in the shared player status.
 */
export function installDownloadMenu(
  player: VideoJsPlayer,
  getSource: () => ResolvedVideoMediaSource,
  getTitle: () => string | null | undefined,
  getTranslation: () => ResolvedPlayerSubtitle | null,
  getPlayerLabel?: () => string | null | undefined,
  setStatus?: (message: string | null) => void,
): { refreshLabel: () => void } | null {
  const playerElement = player.el()
  const controlBar = playerElement?.querySelector<HTMLElement>('.vjs-control-bar')
  if (!(playerElement instanceof HTMLElement) || !controlBar) return null

  const wrapper = document.createElement('div')
  wrapper.className = 'vjs-menu-button vjs-menu-button-popup vjs-download-menu'
  wrapper.dataset.menuAlignment = 'right'
  const toggle = document.createElement('button')
  toggle.type = 'button'
  toggle.className = 'vjs-control vjs-button vjs-download-toggle'
  toggle.setAttribute('aria-haspopup', 'menu')
  toggle.setAttribute('aria-expanded', 'false')
  toggle.append(createIcon('download'))

  const popup = document.createElement('div')
  popup.className = 'vjs-menu'
  const content = document.createElement('div')
  content.className = 'vjs-menu-content'
  content.setAttribute('role', 'menu')
  popup.append(content)
  wrapper.append(toggle, popup)
  controlBar.insertBefore(wrapper, controlBar.querySelector('.vjs-picture-in-picture-control, .vjs-fullscreen-control'))

  const listeners = new AbortController()
  const listenerOptions = { signal: listeners.signal }
  let exportRequest: AbortController | null = null
  let statusTimer: number | undefined
  let hlsDiscovery: AbortController | null = null
  let nativeHlsTracks: SubtitleDownload[] | null = null
  let rendering = false
  let encryptedVideo: HTMLVideoElement | null = null

  /** Updates localized names on the button and on the menu. */
  function refreshLabel() {
    const label = t('player.downloadMenu.label')
    toggle.title = label
    toggle.setAttribute('aria-label', label)
    content.setAttribute('aria-label', label)
  }

  /**
   * Closes the popup, optionally returning keyboard focus to its button.
   * @param restoreFocus Whether focus should return to the control.
   */
  function close(restoreFocus = false) {
    popup.classList.remove('vjs-lock-showing')
    toggle.setAttribute('aria-expanded', 'false')
    if (restoreFocus) toggle.focus()
  }

  /**
   * Clears the player status and queues an action failure in the global error stack.
   * @param key Localized failure message included in the notification details.
   * @param error Original failure, used to explain the cause in the notification details.
   * @param operationKey Localized operation label for the global notification.
   */
  function reportFailure(key: string, error?: unknown, operationKey = 'errors.notifications.operations.playerDownload') {
    if (listeners.signal.aborted) return
    window.clearTimeout(statusTimer)
    setStatus?.(null)
    if (error === undefined || error instanceof ProtectedFrameError) return
    const reason = error instanceof Error ? error.message || error.name : String(error)
    reportFrontendErrorNotification(t(operationKey), `${t(key)} — ${reason}`)
  }

  /**
   * Runs an action and supervises any segmented subtitle export.
   * @param item Activated menu action.
   */
  async function activate(item: DownloadItem) {
    if (item.disabled || (item.assembling && exportRequest)) return
    close(true)
    window.clearTimeout(statusTimer)
    const controller = item.assembling ? new AbortController() : null
    let timeout: number | undefined
    const startedAt = Date.now()
    if (controller) {
      exportRequest = controller
      setStatus?.(t(item.statusKey ?? 'player.downloadMenu.downloadingSubtitles'))
      timeout = window.setTimeout(() => controller.abort(new DOMException('Download export timed out', 'TimeoutError')), 180_000)
    } else if (item.statusKey) {
      setStatus?.(t(item.statusKey))
    }
    try {
      await item.run(controller?.signal ?? listeners.signal)
      if (controller && exportRequest === controller) setStatus?.(null)
      else if (!controller && item.statusKey) {
        statusTimer = window.setTimeout(
          () => setStatus?.(null),
          Math.max(0, MIN_STATUS_VISIBLE_MS - (Date.now() - startedAt)),
        )
      }
    } catch (error) {
      if (controller && exportRequest !== controller) return
      reportFailure(
        error instanceof ProtectedFrameError ? 'player.downloadMenu.screenshotProtected' : item.failureKey,
        error,
        item.errorOperationKey,
      )
    } finally {
      window.clearTimeout(timeout)
      if (controller && exportRequest === controller) exportRequest = null
      controller?.abort()
    }
  }

  /** Collects the actions available for the current source and tracks. */
  const readGroups = () => {
    const video = playerElement.querySelector<HTMLVideoElement>('video.vjs-tech')
    const source = getSource()
    const translation = getTranslation()
    const hlsRenditions = source.transport === 'hls' ? readHlsSubtitleSources(player) : []
    const streamSubtitles = source.transport === 'dash' ? getDashSubtitleDownloads(player)
      : source.transport === 'hls' ? (hlsRenditions.length ? hlsSubtitleDownloads(hlsRenditions, source.src) : nativeHlsTracks ?? [])
        : []
    const subtitles = new Map<string, SubtitleDownload>()
    for (const track of readInjectedSubtitles(player, video, [
      ...source.subtitles, ...(translation ? [translation] : []),
    ])) {
      if (!subtitles.has(track.link)) subtitles.set(track.link, track)
    }
    for (const track of streamSubtitles) {
      if (track.assemble || !subtitles.has(track.link)) subtitles.set(track.link, track)
    }
    const protectedPlayback = Boolean(source.licenseUrl || video?.mediaKeys || (video && encryptedVideo === video))
    const playerDuration = player.duration()
    const duration = typeof playerDuration === 'number' && Number.isFinite(playerDuration) && playerDuration > 0
      ? playerDuration
      : 0
    return buildDownloadGroups(source, getTitle(), getPlayerLabel?.(), video, protectedPlayback, [...subtitles.values()], duration)
  }

  /**
   * Synchronizes button visibility and refreshes an open menu.
   * @param focus Index or text of the item to focus when opening the menu.
   */
  const syncAvailability = (focus?: number | string) => {
    if (listeners.signal.aborted) return
    const groups = readGroups()
    const menuFocused = wrapper.contains(document.activeElement)
    wrapper.hidden = groups.length === 0
    if (wrapper.hidden) {
      close()
      if (menuFocused) controlBar.querySelector<HTMLElement>('.vjs-fullscreen-control')?.focus()
      discoverHlsSubtitles()
      return
    }
    if (focus !== undefined) render(groups, focus)
    else if (toggle.getAttribute('aria-expanded') === 'true') {
      render(groups, document.activeElement?.textContent ?? 0)
    }
  }

  /**
   * Renders available actions and restores the requested focus.
   * @param groups Non-empty groups of download actions.
   * @param focus Index or text of the item to focus.
   */
  const render = (groups: DownloadItem[][], focus: number | string) => {
    rendering = true
    content.replaceChildren()
    groups.forEach((group, index) => {
      if (index > 0) {
        const divider = document.createElement('hr')
        divider.setAttribute('role', 'separator')
        content.append(divider)
      }
      for (const item of group) {
        const button = document.createElement('button')
        button.type = 'button'
        button.className = 'vjs-download-item'
        button.setAttribute('role', 'menuitem')
        button.tabIndex = -1
        if (item.disabled) button.setAttribute('aria-disabled', 'true')
        const label = document.createElement('span')
        label.textContent = item.label
        button.append(createIcon(item.icon, item.iconClass), label)
        button.addEventListener('click', () => { void activate(item) })
        content.append(button)
      }
    })
    content.style.maxHeight = `${Math.max(0, playerElement.clientHeight - controlBar.clientHeight - 8)}px`
    content.style.maxWidth = `${Math.max(0, playerElement.clientWidth - 16)}px`
    popup.classList.add('vjs-lock-showing')
    toggle.setAttribute('aria-expanded', 'true')
    positionControlBarMenu(playerElement, wrapper, 'right')
    const buttons = [...content.querySelectorAll('button')]
    const selected = typeof focus === 'number'
      ? buttons[focus < 0 ? buttons.length - 1 : focus]
      : buttons.find((button) => button.textContent === focus)
    const focused = selected ?? buttons[0]
    focused?.focus()
    rendering = false
  }

  /**
   * Discovers native HLS subtitle renditions when VHS metadata is unavailable.
   */
  function discoverHlsSubtitles() {
    const source = getSource()
    if (source.transport !== 'hls' || nativeHlsTracks || hlsDiscovery || readHlsSubtitleSources(player).length) return
    const controller = new AbortController()
    hlsDiscovery = controller
    const timeout = window.setTimeout(() => controller.abort(), 10_000)
    void discoverNativeHlsSubtitles(source.src, controller.signal).then((tracks) => {
      if (controller.signal.aborted || hlsDiscovery !== controller) return
      nativeHlsTracks = tracks
      syncAvailability()
    }).catch(() => {
      if (hlsDiscovery === controller) nativeHlsTracks = []
    }).finally(() => {
      window.clearTimeout(timeout)
      if (hlsDiscovery === controller) hlsDiscovery = null
    })
  }

  /**
   * Opens the menu and requests any additional native HLS tracks.
   * @param last Whether to focus the last action first.
   */
  function open(last = false) {
    syncAvailability(last ? -1 : 0)
    discoverHlsSubtitles()
  }

  toggle.addEventListener('click', () => {
    if (toggle.getAttribute('aria-expanded') === 'true') close(true)
    else open()
  })
  wrapper.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && toggle.getAttribute('aria-expanded') === 'true') {
      event.preventDefault()
      event.stopPropagation()
      close(true)
      return
    }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
    const items = Array.from(content.querySelectorAll<HTMLButtonElement>('button'))
    const current = items.indexOf(document.activeElement as HTMLButtonElement)
    if (current < 0 && event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
    event.preventDefault()
    event.stopPropagation()
    if (current < 0) {
      open(event.key === 'ArrowUp')
      return
    }
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1
      : (current + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length
    items[next]?.focus()
  })
  wrapper.addEventListener('focusout', (event) => {
    if (!rendering && !wrapper.contains(event.relatedTarget as Node | null)) close()
  })
  document.addEventListener('pointerdown', (event) => {
    if (!wrapper.contains(event.target as Node)) close()
  }, listenerOptions)
  playerElement.addEventListener('encrypted', (event) => {
    if (!(event.target instanceof HTMLVideoElement) || encryptedVideo === event.target) return
    encryptedVideo = event.target
    syncAvailability()
  }, { ...listenerOptions, capture: true })
  const closeOnLoad = () => {
    encryptedVideo = null
    close(content.contains(document.activeElement))
    hlsDiscovery?.abort()
    hlsDiscovery = null
    nativeHlsTracks = null
    exportRequest?.abort()
    exportRequest = null
    setStatus?.(null)
  }
  const syncTextTracks = () => queueMicrotask(() => syncAvailability())
  type TrackEvents = Pick<EventTarget, 'addEventListener' | 'removeEventListener'>
  let textTracks: TrackEvents | undefined

  /** Observes tracks on the active tech, including translations added or removed later. */
  function observeTextTracks() {
    const current = player.textTracks?.() as unknown as TrackEvents | undefined
    if (textTracks === current) return
    textTracks?.removeEventListener('addtrack', syncTextTracks)
    textTracks?.removeEventListener('removetrack', syncTextTracks)
    textTracks = current
    textTracks?.addEventListener('addtrack', syncTextTracks)
    textTracks?.addEventListener('removetrack', syncTextTracks)
  }

  const onMetadata = () => {
    observeTextTracks()
    syncAvailability()
  }
  player.on('loadstart', () => {
    closeOnLoad()
    onMetadata()
  })
  player.on('loadedmetadata', onMetadata)
  player.on('loadeddata', onMetadata)
  player.on('durationchange', onMetadata)
  player.on('dispose', () => {
    listeners.abort()
    closeOnLoad()
    textTracks?.removeEventListener('addtrack', syncTextTracks)
    textTracks?.removeEventListener('removetrack', syncTextTracks)
    window.clearTimeout(statusTimer)
    wrapper.remove()
  })
  refreshLabel()
  onMetadata()
  return { refreshLabel }
}
