import type { ResolvedVideoChapter } from '@/services/players'

import {
  installAutoSkipChapters,
  installChapterOverlay,
  installChapterSegments,
  installSkipChapterButton,
  SKIP_CHAPTER_BUTTON_TYPES,
  type SkipChapterType,
} from '@/composables/video/video-js-media-renderer/chapters'
import type {
  VideoJsDashManifestPeriod,
  VideoJsDashMediaPlayerHandle,
  VideoJsDashPeriodInfo,
  VideoJsPlayer,
} from '@/composables/video/video-js-media-renderer/types'

import { DASH_EVENTS } from './events'
import { getDashMediaPlayer } from './source'

/** Chapter type applied to DASH advertising periods detected in the manifest. */
export const DASH_AD_CHAPTER_TYPE = 'ads'

/** dash.js events after which the manifest periods may become readable. */
const DASH_PERIOD_CHAPTER_EVENTS = [
  DASH_EVENTS.manifestLoaded,
  DASH_EVENTS.playbackMetadataLoaded,
  DASH_EVENTS.streamInitialized,
]

/**
 * Reads the regular periods known by the dash.js adapter.
 *
 * @param mediaPlayer dash.js MediaPlayer bound to the active source.
 * @returns Ordered periods, or `null` while the manifest is not parsed yet.
 */
function readDashRegularPeriods(mediaPlayer: VideoJsDashMediaPlayerHandle): VideoJsDashPeriodInfo[] | null {
  try {
    const periods = mediaPlayer.getDashAdapter?.()?.getRegularPeriods?.()
    return Array.isArray(periods) && periods.length > 0 ? periods : null
  } catch {
    return null
  }
}

/**
 * Extracts the base URL signature of one raw manifest period.
 *
 * @param period Raw period object parsed from the manifest by dash.js.
 * @returns Base URL text of the period, or an empty string when absent.
 */
function readPeriodBaseUrl(period: VideoJsDashManifestPeriod | null | undefined): string {
  const entries = period?.BaseURL_asArray
  const firstEntry = Array.isArray(entries) ? entries[0] : undefined

  if (typeof firstEntry === 'string') {
    return firstEntry
  }

  const text = firstEntry?.__text
  if (typeof text === 'string' && text.length > 0) {
    return text
  }

  return period?.baseUri ?? ''
}

/**
 * Builds chapters for the DASH advertising periods of the active manifest.
 *
 * Advertising periods are detected structurally: periods are grouped by their
 * base URL, and every group that is shorter than the longest one (the program)
 * is reported as advertising. The method returns `null` while the manifest
 * periods are not available yet so callers can retry after the manifest load.
 *
 * @param mediaPlayer dash.js MediaPlayer bound to the active source.
 * @returns Advertising chapters in presentation time, or `null` when the manifest is not parsed yet.
 */
export function readDashAdPeriodChapters(
  mediaPlayer: VideoJsDashMediaPlayerHandle,
): ResolvedVideoChapter[] | null {
  const periods = readDashRegularPeriods(mediaPlayer)

  if (periods === null) {
    return null
  }

  if (periods.length <= 1) {
    return []
  }

  const rawPeriods = periods[0]?.mpd?.manifest?.Period_asArray
  const entries = periods.map((period, index) => {
    const start = typeof period.start === 'number' && Number.isFinite(period.start) ? period.start : 0
    const duration = typeof period.duration === 'number' && Number.isFinite(period.duration) && period.duration > 0
      ? period.duration
      : null
    const nextStart = periods[index + 1]?.start
    const end = duration !== null
      ? start + duration
      : typeof nextStart === 'number' && Number.isFinite(nextStart)
        ? nextStart
        : Number.POSITIVE_INFINITY

    return {
      baseUrl: readPeriodBaseUrl(Array.isArray(rawPeriods) ? rawPeriods[index] : undefined),
      start,
      end,
    }
  })

  const durationByBaseUrl = new Map<string, number>()
  entries.forEach((entry) => {
    durationByBaseUrl.set(entry.baseUrl, (durationByBaseUrl.get(entry.baseUrl) ?? 0) + (entry.end - entry.start))
  })

  let programBaseUrl = ''
  let longestGroupDuration = -1
  durationByBaseUrl.forEach((total, baseUrl) => {
    if (total > longestGroupDuration) {
      longestGroupDuration = total
      programBaseUrl = baseUrl
    }
  })

  return entries
    .filter((entry) => entry.baseUrl !== programBaseUrl)
    .map((entry) => ({
      start: entry.start,
      end: entry.end,
      title: '',
      type: DASH_AD_CHAPTER_TYPE,
    }))
}

/**
 * Merges backend chapters with the DASH advertising chapters, ordered by start time.
 *
 * @param baseChapters Chapters resolved from the backend metadata.
 * @param adChapters Chapters derived from the DASH advertising periods.
 * @returns Ordered chapter list.
 */
function mergeChaptersByStartTime(
  baseChapters: ResolvedVideoChapter[],
  adChapters: ResolvedVideoChapter[],
): ResolvedVideoChapter[] {
  return [...baseChapters, ...adChapters].sort((a, b) => a.start - b.start)
}

/**
 * Options accepted by the DASH chapter installer.
 */
export interface DashPeriodChaptersOptions {
  /**
   * Reads whether automatic skipping is enabled for a chapter type.
   */
  isAutoskipEnabled: (chapterType: SkipChapterType) => boolean
}

/**
 * Installs the chapter overlays including the DASH advertising periods.
 *
 * The DASH engine is attached asynchronously after the source is applied, and
 * dash.js only exposes the manifest periods after the manifest is loaded. The
 * installation is therefore deferred: it is armed immediately, retries on the
 * next tick and on `loadedmetadata`, and reads the periods as soon as the
 * engine reports a manifest. When the source is not played through dash.js,
 * the caller falls back to the static chapter installation.
 *
 * @param player Video.js player bound to the DASH source.
 * @param baseChapters Chapters resolved from the backend metadata.
 * @param options Auto-skip readers shared with the static chapter installation.
 * @returns `true` when the DASH engine owns the chapter installation.
 */
export function installDashPeriodChapters(
  player: VideoJsPlayer,
  baseChapters: ResolvedVideoChapter[],
  options: DashPeriodChaptersOptions,
): boolean {
  let chaptersInstalled = false
  let engineAttached = false
  let pendingRetryTimer: number | null = null
  /** dash.js MediaPlayer reference once the engine is attached, or `null`. */
  let attachedMediaPlayer: VideoJsDashMediaPlayerHandle | null = null

  const removePendingTriggers = () => {
    player.off('loadedmetadata', handleLoadedMetadata)
    player.off('dispose', handleDispose)
    if (pendingRetryTimer !== null) {
      window.clearTimeout(pendingRetryTimer)
      pendingRetryTimer = null
    }
  }

  const removeDashListeners = () => {
    if (!attachedMediaPlayer) {
      return
    }

    DASH_PERIOD_CHAPTER_EVENTS.forEach((eventName) => {
      attachedMediaPlayer?.off(eventName, handleManifestEvent)
    })
    attachedMediaPlayer = null
  }

  const handleDispose = () => {
    removePendingTriggers()
    removeDashListeners()
  }

  const handleLoadedMetadata = () => {
    tryInstallChapters()
  }

  const handleManifestEvent = () => {
    tryInstallChapters()
  }

  const attachEngine = (mediaPlayer: VideoJsDashMediaPlayerHandle) => {
    engineAttached = true
    attachedMediaPlayer = mediaPlayer
    DASH_PERIOD_CHAPTER_EVENTS.forEach((eventName) => {
      mediaPlayer.on(eventName, handleManifestEvent)
    })
  }

  const tryAttachEngine = () => {
    if (engineAttached) {
      return
    }

    const mediaPlayer = getDashMediaPlayer(player)

    if (mediaPlayer) {
      attachEngine(mediaPlayer)
    }
  }

  const tryInstallChapters = () => {
    if (chaptersInstalled) {
      return
    }

    const mediaPlayer = getDashMediaPlayer(player)

    if (!mediaPlayer) {
      return
    }

    const adChapters = readDashAdPeriodChapters(mediaPlayer)

    if (adChapters === null) {
      return
    }

    chaptersInstalled = true
    removePendingTriggers()
    removeDashListeners()

    const chapters = mergeChaptersByStartTime(baseChapters, adChapters)

    if (chapters.length > 0) {
      installChapterOverlay(player, chapters)
      installChapterSegments(player, chapters)
    }

    if (adChapters.length > 0) {
      for (const chapterType of SKIP_CHAPTER_BUTTON_TYPES) {
        const hasTypedChapters = adChapters.some((chapter) => chapter.type === chapterType)
        if (hasTypedChapters) {
          installSkipChapterButton(player, chapters, chapterType)
        }
      }
    }

    installAutoSkipChapters(player, {
      chapters,
      isAutoskipEnabled: options.isAutoskipEnabled,
    })
  }

  player.on('loadedmetadata', handleLoadedMetadata)
  player.on('dispose', handleDispose)

  // The DASH engine is created when the source handler processes the source,
  // which can happen after this call; retry once on the next tick.
  pendingRetryTimer = window.setTimeout(() => {
    pendingRetryTimer = null
    tryAttachEngine()
    tryInstallChapters()
  }, 0)

  tryAttachEngine()
  tryInstallChapters()

  return true
}
