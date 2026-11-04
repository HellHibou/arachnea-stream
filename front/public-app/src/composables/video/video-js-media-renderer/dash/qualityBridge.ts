import {
  normalizeQualityPreferenceLabel,
  resolveQualityPreferenceToken,
  syncDisplayedQualityPreference,
} from '@/composables/video/video-js-media-renderer/quality'
import type {
  QualityLevelHandle,
  QualityLevelListHandle,
  QualityLevelRepresentation,
  VideoJsDashBitrateInfo,
  VideoJsDashMediaPlayerHandle,
  VideoJsPlayer,
} from '@/composables/video/video-js-media-renderer/types'

import { DASH_EVENTS } from './events'
import { getDashMediaPlayer } from './source'

/** dash.js media type used for video quality selection. */
const VIDEO_MEDIA_TYPE = 'video'

/** Prefix applied to the quality levels created from dash.js representations. */
const DASH_QUALITY_LEVEL_ID_PREFIX = 'dash-video-'

/** Video height in pixels from which a rendition is considered high definition. */
const HIGH_DEFINITION_HEIGHT = 720

/** dash.js events that invalidate the available video representations. */
const DASH_REPRESENTATION_EVENTS = [
  DASH_EVENTS.manifestLoaded,
  DASH_EVENTS.playbackMetadataLoaded,
  DASH_EVENTS.periodSwitchCompleted,
  DASH_EVENTS.streamInitialized,
]

/** dash.js events after which Video.js track lists may have been rebuilt. */
const DASH_TRACK_LIST_EVENTS = [
  DASH_EVENTS.playbackMetadataLoaded,
  DASH_EVENTS.periodSwitchCompleted,
  'allTextTracksAdded',
]

/** Options accepted when installing the DASH quality bridge. */
interface InstallDashQualityBridgeOptions {
  /** Video.js player bound to the DASH source. */
  player: VideoJsPlayer
  /** Quality label persisted for the previous source, or `null` for automatic quality. */
  preferredQuality: string | null
  /** Reapplies audio and subtitle preferences after dash.js renews the track lists. */
  restoreTrackPreferences?: () => void
}

/** Quality bridge attached to one active DASH source. */
export interface DashQualityBridge {
  /**
   * Applies one quality preference label to the dash.js MediaPlayer.
   *
   * @param qualityLabel Quality label selected in the Video.js menu, or `null` for automatic quality.
   */
  applyQualityPreference: (qualityLabel: string | null) => void
  /** Removes the dash.js listeners and the quality levels owned by the bridge. */
  dispose: () => void
}

/**
 * Reads the video representations exposed by dash.js for the active period.
 *
 * @param mediaPlayer dash.js MediaPlayer bound to the active source.
 * @returns Available video representations, or an empty array while dash.js is not initialized.
 */
function readDashVideoLevels(mediaPlayer: VideoJsDashMediaPlayerHandle): VideoJsDashBitrateInfo[] {
  try {
    const levels = mediaPlayer.getBitrateInfoListFor(VIDEO_MEDIA_TYPE)
    return Array.isArray(levels) ? levels : []
  } catch {
    return []
  }
}

/**
 * Reads the quality index currently rendered by dash.js.
 *
 * @param mediaPlayer dash.js MediaPlayer bound to the active source.
 * @returns Current video quality index, or `-1` while dash.js is not initialized.
 */
function readDashVideoQualityIndex(mediaPlayer: VideoJsDashMediaPlayerHandle): number {
  try {
    const qualityIndex = mediaPlayer.getQualityFor(VIDEO_MEDIA_TYPE)
    return Number.isFinite(qualityIndex) ? qualityIndex : -1
  } catch {
    return -1
  }
}

/**
 * Enables or disables adaptive bitrate selection for the video track.
 *
 * @param mediaPlayer dash.js MediaPlayer bound to the active source.
 * @param enabled Whether dash.js should keep adapting the video quality.
 */
function setDashAutomaticQuality(mediaPlayer: VideoJsDashMediaPlayerHandle, enabled: boolean) {
  mediaPlayer.updateSettings({
    streaming: {
      abr: {
        autoSwitchBitrate: {
          video: enabled,
        },
      },
    },
  })
}

/**
 * Resolves the dash.js quality index matching one normalized quality preference token.
 *
 * @param levels Video representations ordered by increasing bitrate.
 * @param qualityPreferenceToken Normalized token derived from the selected menu label.
 * @returns Matching quality index, or `-1` when the token cannot be mapped to a rendition.
 */
function resolveDashQualityIndex(
  levels: VideoJsDashBitrateInfo[],
  qualityPreferenceToken: string,
): number {
  if (levels.length === 0) {
    return -1
  }

  if (/^\d+$/.test(qualityPreferenceToken)) {
    const preferredHeight = Number.parseInt(qualityPreferenceToken, 10)
    let closestIndex = -1
    let closestDistance = Number.POSITIVE_INFINITY

    levels.forEach((level, index) => {
      const height = level.height

      if (typeof height !== 'number' || !Number.isFinite(height)) {
        return
      }

      const distance = Math.abs(height - preferredHeight)

      if (distance < closestDistance) {
        closestDistance = distance
        closestIndex = index
      }
    })

    return closestIndex
  }

  const isHighDefinition = qualityPreferenceToken === 'HD'
  const matchingIndices = levels
    .map((level, index) => {
      const height = level.height

      if (typeof height !== 'number' || !Number.isFinite(height)) {
        return -1
      }

      const matchesBucket = height >= HIGH_DEFINITION_HEIGHT

      return isHighDefinition === matchesBucket ? index : -1
    })
    .filter((index) => index >= 0)

  if (matchingIndices.length > 0) {
    return matchingIndices[matchingIndices.length - 1] ?? -1
  }

  return isHighDefinition ? levels.length - 1 : 0
}

/**
 * Installs the dash.js quality bridge for one DASH source.
 *
 * The bridge publishes the video representations of the active period through the
 * Video.js quality level list and translates quality selections back into dash.js
 * commands. It returns `null` when the DASH source handler is not ready.
 *
 * @param options Player bound to the source and quality preference to restore.
 * @returns Bridge handle, or `null` when dash.js is unavailable.
 */
export function installDashQualityBridge(
  options: InstallDashQualityBridgeOptions,
): DashQualityBridge | null {
  const { player, restoreTrackPreferences } = options
  const mediaPlayer = getDashMediaPlayer(player)
  const qualityLevels = typeof player.qualityLevels === 'function'
    ? (player.qualityLevels() as QualityLevelListHandle)
    : null

  if (!mediaPlayer || !qualityLevels?.addQualityLevel || !qualityLevels.removeQualityLevel) {
    return null
  }

  /** Quality levels currently owned by the bridge. */
  let ownedLevels: QualityLevelHandle[] = []
  /** Signature of the representations currently published to Video.js. */
  let representationSignature = ''
  /** Quality label currently applied to dash.js, or `null` for automatic quality. */
  let qualityLabel = normalizeQualityPreferenceLabel(options.preferredQuality)
  /** Pending timer used to coalesce consecutive dash.js events. */
  let syncTimer: number | null = null
  /** Pending timer used to collect one quality-menu enabled-state update. */
  let menuSelectionTimer: number | null = null
  /** Quality level indices currently enabled through the Video.js quality menu. */
  let enabledMenuLevelIndices = new Set<number>()
  /** Pending timer used to wait for the source handler to rebuild Video.js track lists. */
  let trackPreferenceRestoreTimer: number | null = null

  const setSelectedIndex = (index: number) => {
    if (qualityLevels.selectedIndex_ === index) {
      return
    }

    qualityLevels.selectedIndex_ = index
    qualityLevels.trigger?.({ type: 'change', selectedIndex: index })
  }

  const findListIndexForDashQuality = (levels: VideoJsDashBitrateInfo[], dashQualityIndex: number) => {
    return levels.findIndex((level, index) => (level.qualityIndex ?? index) === dashQualityIndex)
  }

  const selectQualityIndex = (levels: VideoJsDashBitrateInfo[], listIndex: number) => {
    const level = levels[listIndex]

    if (!level) {
      return
    }

    const dashQualityIndex = level.qualityIndex ?? listIndex
    setDashAutomaticQuality(mediaPlayer, false)

    try {
      mediaPlayer.setQualityFor(VIDEO_MEDIA_TYPE, dashQualityIndex)
    } catch {
      return
    }

    setSelectedIndex(listIndex)
  }

  const applyQualityPreference = (nextQualityLabel: string | null) => {
    qualityLabel = normalizeQualityPreferenceLabel(nextQualityLabel)

    const levels = readDashVideoLevels(mediaPlayer)

    if (levels.length === 0) {
      return
    }

    const qualityPreferenceToken = resolveQualityPreferenceToken(qualityLabel)

    if (!qualityPreferenceToken) {
      setDashAutomaticQuality(mediaPlayer, true)
      setSelectedIndex(findListIndexForDashQuality(levels, readDashVideoQualityIndex(mediaPlayer)))
      return
    }

    const qualityIndex = resolveDashQualityIndex(levels, qualityPreferenceToken)

    if (qualityIndex < 0) {
      // Keep the preference for the next period but let dash.js adapt until then.
      setDashAutomaticQuality(mediaPlayer, true)
      return
    }

    selectQualityIndex(levels, qualityIndex)
  }

  const applyQualityMenuSelection = () => {
    menuSelectionTimer = null

    const levels = readDashVideoLevels(mediaPlayer)

    if (levels.length === 0 || enabledMenuLevelIndices.size === 0) {
      return
    }

    if (enabledMenuLevelIndices.size === levels.length) {
      qualityLabel = null
      setDashAutomaticQuality(mediaPlayer, true)
      return
    }

    if (enabledMenuLevelIndices.size !== 1) {
      return
    }

    const [listIndex] = enabledMenuLevelIndices

    if (typeof listIndex !== 'number') {
      return
    }

    const selectedLevel = levels[listIndex]

    if (!selectedLevel) {
      return
    }

    qualityLabel = typeof selectedLevel.height === 'number' && Number.isFinite(selectedLevel.height)
      ? `${selectedLevel.height}p`
      : null
    selectQualityIndex(levels, listIndex)
  }

  const updateQualityMenuEnabledState = (index: number, enabled?: boolean): boolean => {
    if (typeof enabled !== 'boolean') {
      return enabledMenuLevelIndices.has(index)
    }

    if (enabled) {
      enabledMenuLevelIndices.add(index)
    } else {
      enabledMenuLevelIndices.delete(index)
    }

    if (menuSelectionTimer === null) {
      menuSelectionTimer = window.setTimeout(applyQualityMenuSelection, 0)
    }

    return enabled
  }

  const syncQualityLevels = () => {
    const levels = readDashVideoLevels(mediaPlayer)

    if (levels.length === 0) {
      return
    }

    const nextSignature = levels
      .map((level, index) => {
        const bitrate = level.bitrate ?? 0
        const width = level.width ?? 0
        const height = level.height ?? 0
        return `${index}:${bitrate}:${width}x${height}`
      })
      .join('|')

    if (nextSignature !== representationSignature) {
      representationSignature = nextSignature
      enabledMenuLevelIndices = new Set(
        levels.map((_, index) => index),
      )

      ownedLevels.forEach((level) => {
        qualityLevels.removeQualityLevel?.(level)
      })

      ownedLevels = levels
        .map((level, index) => {
          const representation: QualityLevelRepresentation = {
            id: `${DASH_QUALITY_LEVEL_ID_PREFIX}${level.qualityIndex ?? index}`,
            width: level.width ?? undefined,
            height: level.height ?? undefined,
            bandwidth: level.bitrate ?? undefined,
            enabled: (enabled) => updateQualityMenuEnabledState(index, enabled),
          }

          return qualityLevels.addQualityLevel?.(representation)
        })
        .filter((level): level is QualityLevelHandle => Boolean(level))
    }

    syncDisplayedQualityPreference(player, qualityLabel, { treatMissingMenuAsUnavailable: false })
    applyQualityPreference(qualityLabel)
  }

  const handleRepresentationChange = () => {
    if (syncTimer !== null) {
      return
    }

    // dash.js emits several events per period switch; coalesce them into one refresh.
    syncTimer = window.setTimeout(() => {
      syncTimer = null
      syncQualityLevels()
    }, 0)
  }

  const handleTrackListRenewal = () => {
    if (!restoreTrackPreferences || trackPreferenceRestoreTimer !== null) {
      return
    }

    trackPreferenceRestoreTimer = window.setTimeout(() => {
      trackPreferenceRestoreTimer = null
      restoreTrackPreferences()
    }, 0)
  }

  const handleQualityChangeRendered = (event: Record<string, unknown>) => {
    if (event.mediaType !== VIDEO_MEDIA_TYPE) {
      return
    }

    const newQuality = event.newQuality

    if (typeof newQuality === 'number' && Number.isFinite(newQuality)) {
      const listIndex = findListIndexForDashQuality(readDashVideoLevels(mediaPlayer), newQuality)

      setSelectedIndex(listIndex)
    }
  }

  DASH_REPRESENTATION_EVENTS.forEach((eventName) => {
    mediaPlayer.on(eventName, handleRepresentationChange)
  })
  DASH_TRACK_LIST_EVENTS.forEach((eventName) => {
    mediaPlayer.on(eventName, handleTrackListRenewal)
  })
  mediaPlayer.on(DASH_EVENTS.qualityChangeRendered, handleQualityChangeRendered)

  // The manifest may already be parsed when the bridge is installed on a reused player.
  handleRepresentationChange()

  const dispose = () => {
    if (syncTimer !== null) {
      window.clearTimeout(syncTimer)
      syncTimer = null
    }

    if (menuSelectionTimer !== null) {
      window.clearTimeout(menuSelectionTimer)
      menuSelectionTimer = null
    }

    if (trackPreferenceRestoreTimer !== null) {
      window.clearTimeout(trackPreferenceRestoreTimer)
      trackPreferenceRestoreTimer = null
    }

    DASH_REPRESENTATION_EVENTS.forEach((eventName) => {
      mediaPlayer.off(eventName, handleRepresentationChange)
    })
    DASH_TRACK_LIST_EVENTS.forEach((eventName) => {
      mediaPlayer.off(eventName, handleTrackListRenewal)
    })
    mediaPlayer.off(DASH_EVENTS.qualityChangeRendered, handleQualityChangeRendered)

    ownedLevels.forEach((level) => {
      qualityLevels.removeQualityLevel?.(level)
    })
    ownedLevels = []
    representationSignature = ''
    enabledMenuLevelIndices.clear()
    setSelectedIndex(-1)
  }

  return { applyQualityPreference, dispose }
}
