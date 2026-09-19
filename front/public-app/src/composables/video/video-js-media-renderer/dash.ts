import videojsDashModule from 'videojs-contrib-dash'

import { DASH_MIME_TYPE } from '@/services/players'
import type { ResolvedVideoChapter, ResolvedVideoMediaSource } from '@/services/players'

import { installChapterOverlay, installChapterSegments, installSkipChapterButton } from '@/composables/video/video-js-media-renderer/chapters'
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
  VideoJsDashKeySystemOption,
  VideoJsDashMediaPlayerHandle,
  VideoJsDashManifestPeriod,
  VideoJsDashPeriodInfo,
  VideoJsPlayer,
} from '@/composables/video/video-js-media-renderer/types'

/** Widevine key system identifier expected by the DASH source handler. */
const WIDEVINE_KEY_SYSTEM = 'com.widevine.alpha'

/** dash.js media type used for video quality selection. */
const VIDEO_MEDIA_TYPE = 'video'

/** DASH scheme declaring mutually switchable adaptation sets (AWS MediaPackage split ladders). */
const ADAPTATION_SET_SWITCHING_SCHEME = 'urn:mpeg:dash:adaptation-set-switching:2016'

/** Minimal surface of the videojs-contrib-dash source handler used for lifecycle hooks. */
interface DashSourceHandlerHooks {
  hook: (
    lifecycle: 'beforeinitialize',
    hook: (player: VideoJsPlayer, mediaPlayer: VideoJsDashMediaPlayerHandle) => void,
  ) => void
}

/** Loose view of one parsed manifest supplemental property. */
interface DashManifestSupplementalPropertyView {
  schemeIdUri?: unknown
  value?: unknown
}

/** Loose view of one parsed manifest adaptation set. */
interface DashManifestAdaptationSetView {
  id?: unknown
  mimeType?: unknown
  SegmentTemplate?: unknown
  SegmentTemplate_asArray?: unknown
  Representation_asArray?: unknown
  SupplementalProperty_asArray?: unknown
}

/** Loose view of one parsed manifest period. */
interface DashManifestPeriodView {
  AdaptationSet_asArray?: unknown
}

/** Loose view of the parsed DASH manifest object. */
interface DashManifestDocumentView {
  Period_asArray?: unknown
}

/** Module default export typed for the hook API. */
const videojsDash = videojsDashModule as unknown as DashSourceHandlerHooks

/** Prefix applied to the quality levels created from dash.js representations. */
const DASH_QUALITY_LEVEL_ID_PREFIX = 'dash-video-'

/** Prefix used by the DASH source handler for index-based audio track identifiers. */
const DASH_AUDIO_TRACK_ID_PREFIX = 'dash-audio-'

/** Video height in pixels from which a rendition is considered high definition. */
const HIGH_DEFINITION_HEIGHT = 720

/**
 * dash.js events consumed by the quality bridge.
 *
 * The dash.js build bundled inside the source handler is not importable, so the
 * event names are declared as literals instead of using the events enum.
 */
const DASH_EVENTS = {
  manifestLoaded: 'manifestLoaded',
  periodSwitchCompleted: 'periodSwitchCompleted',
  playbackMetadataLoaded: 'playbackMetaDataLoaded',
  qualityChangeRendered: 'qualityChangeRendered',
  streamInitialized: 'streamInitialized',
} as const

/** dash.js events that invalidate the available video representations. */
const DASH_REPRESENTATION_EVENTS = [
  DASH_EVENTS.manifestLoaded,
  DASH_EVENTS.playbackMetadataLoaded,
  DASH_EVENTS.periodSwitchCompleted,
  DASH_EVENTS.streamInitialized,
]

/**
 * Reads one parsed manifest property as an array when present.
 *
 * @param value Parsed manifest property value.
 * @returns Array value, or an empty array when the property is absent.
 */
function readManifestArray<T>(value: unknown): T[] {
  return Array.isArray(value) ? (value as T[]) : []
}

/**
 * Reads the identifier of one parsed adaptation set.
 *
 * @param adaptationSet Parsed adaptation set object.
 * @returns Identifier as text, or `null` when the set has no identifier.
 */
function readManifestAdaptationSetId(adaptationSet: DashManifestAdaptationSetView): string | null {
  return typeof adaptationSet.id === 'string' || typeof adaptationSet.id === 'number'
    ? String(adaptationSet.id)
    : null
}

/**
 * Indicates whether one parsed adaptation set carries video representations.
 *
 * @param adaptationSet Parsed adaptation set object.
 * @returns `true` when the set holds video media.
 */
function isVideoManifestAdaptationSet(adaptationSet: DashManifestAdaptationSetView): boolean {
  if (typeof adaptationSet.mimeType === 'string' && adaptationSet.mimeType.startsWith('video/')) {
    return true
  }

  return readManifestArray<Record<string, unknown>>(adaptationSet.Representation_asArray).some(
    (representation) =>
      typeof representation.width === 'number' || typeof representation.height === 'number',
  )
}

/**
 * Indicates whether one parsed adaptation set declares its own segment template.
 *
 * Merging sets that carry a shared segment template is unsafe because the template
 * would silently apply to representations owned by other sets.
 *
 * @param adaptationSet Parsed adaptation set object.
 * @returns `true` when the set declares a segment template at set level.
 */
function hasOwnManifestSegmentTemplate(adaptationSet: DashManifestAdaptationSetView): boolean {
  return Array.isArray(adaptationSet.SegmentTemplate_asArray) ||
    adaptationSet.SegmentTemplate !== undefined
}

/**
 * Reads the identifiers of the adaptation sets declared switchable with the given one.
 *
 * @param adaptationSet Parsed adaptation set object.
 * @returns Identifiers of the switchable sibling adaptation sets.
 */
function readSwitchableManifestAdaptationSetIds(
  adaptationSet: DashManifestAdaptationSetView,
): string[] {
  return readManifestArray<DashManifestSupplementalPropertyView>(
    adaptationSet.SupplementalProperty_asArray,
  ).flatMap((property) => {
    if (property.schemeIdUri !== ADAPTATION_SET_SWITCHING_SCHEME) {
      return []
    }

    return typeof property.value === 'string'
      ? property.value.split(',').map((id) => id.trim()).filter(Boolean)
      : []
  })
}

/**
 * Reads the bandwidth of one parsed representation, ordered last when unknown.
 *
 * @param representation Parsed representation object.
 * @returns Bandwidth value used for ascending ordering.
 */
function readManifestRepresentationBandwidth(representation: Record<string, unknown>): number {
  return typeof representation.bandwidth === 'number' && Number.isFinite(representation.bandwidth)
    ? representation.bandwidth
    : Number.POSITIVE_INFINITY
}

/**
 * Merges the mutually switchable video adaptation sets of every manifest period into one set.
 *
 * AWS MediaPackage manifests may split the video ladder across adaptation sets declared
 * with `urn:mpeg:dash:adaptation-set-switching:2016`. dash.js does not implement this
 * scheme: it exposes only the representations of one set at a time, hiding the other
 * ladder rungs from the quality menu and from adaptive bitrate selection. Merging each
 * declared group restores the full ladder without touching the other manifest features.
 *
 * @param manifest Parsed DASH manifest object, mutated in place.
 */
export function mergeSwitchableVideoManifestAdaptationSets(manifest: unknown): void {
  const periods = readManifestArray<DashManifestPeriodView>(
    (manifest as DashManifestDocumentView | null | undefined)?.Period_asArray,
  )

  periods.forEach((period) => {
    const adaptationSets = readManifestArray<DashManifestAdaptationSetView>(
      period.AdaptationSet_asArray,
    )

    if (adaptationSets.length < 2) {
      return
    }

    const idBySet = new Map<DashManifestAdaptationSetView, string>()
    const setsById = new Map<string, DashManifestAdaptationSetView>()

    adaptationSets.forEach((adaptationSet) => {
      const id = readManifestAdaptationSetId(adaptationSet)

      if (id !== null) {
        idBySet.set(adaptationSet, id)
        setsById.set(id, adaptationSet)
      }
    })

    const switchableIdsBySet = new Map<DashManifestAdaptationSetView, string[]>()

    idBySet.forEach((id, adaptationSet) => {
      const switchableIds = readSwitchableManifestAdaptationSetIds(adaptationSet)
        .filter((switchableId) => switchableId !== id && setsById.has(switchableId))

      if (switchableIds.length > 0) {
        switchableIdsBySet.set(adaptationSet, switchableIds)
      }
    })

    if (switchableIdsBySet.size === 0) {
      return
    }

    // Union-find over the declared identifiers groups the sets that can switch together.
    const groupRootById = new Map<string, string>()
    const findGroupRoot = (id: string): string => {
      let root = id
      let parent = groupRootById.get(root)

      while (parent !== undefined && parent !== root) {
        root = parent
        parent = groupRootById.get(root)
      }

      return root
    }

    switchableIdsBySet.forEach((switchableIds, adaptationSet) => {
      const id = idBySet.get(adaptationSet) as string

      groupRootById.set(id, id)
      switchableIds.forEach((switchableId) => {
        groupRootById.set(switchableId, switchableId)
        const adaptationSetRoot = findGroupRoot(id)
        const switchableRoot = findGroupRoot(switchableId)

        if (adaptationSetRoot !== switchableRoot) {
          groupRootById.set(adaptationSetRoot, switchableRoot)
        }
      })
    })

    const membersByRoot = new Map<string, DashManifestAdaptationSetView[]>()

    switchableIdsBySet.forEach((_, adaptationSet) => {
      const root = findGroupRoot(idBySet.get(adaptationSet) as string)
      const members = membersByRoot.get(root) ?? []

      members.push(adaptationSet)
      membersByRoot.set(root, members)
    })

    const removedSets = new Set<DashManifestAdaptationSetView>()

    membersByRoot.forEach((members) => {
      if (members.length < 2 || members.some((member) => !isVideoManifestAdaptationSet(member))) {
        return
      }

      if (members.some((member) => hasOwnManifestSegmentTemplate(member))) {
        return
      }

      // Keep the set with the most representations as the merge target.
      const primary = [...members].sort(
        (first, second) =>
          readManifestArray<unknown>(first.Representation_asArray).length -
          readManifestArray<unknown>(second.Representation_asArray).length,
      ).pop() as DashManifestAdaptationSetView

      members
        .filter((member) => member !== primary)
        .forEach((member) => {
          primary.Representation_asArray = [
            ...readManifestArray<unknown>(primary.Representation_asArray),
            ...readManifestArray<unknown>(member.Representation_asArray),
          ]
          removedSets.add(member)
        })

      // dash.js maps quality indices from the representation order, which must stay
      // sorted by increasing bandwidth for the ABR and the quality menu.
      primary.Representation_asArray = readManifestArray<Record<string, unknown>>(
        primary.Representation_asArray,
      ).sort(
        (first, second) =>
          readManifestRepresentationBandwidth(first) - readManifestRepresentationBandwidth(second),
      )

      primary.SupplementalProperty_asArray = readManifestArray<DashManifestSupplementalPropertyView>(
        primary.SupplementalProperty_asArray,
      ).filter((property) => property.schemeIdUri !== ADAPTATION_SET_SWITCHING_SCHEME)
    })

    if (removedSets.size > 0) {
      period.AdaptationSet_asArray = adaptationSets.filter((set) => !removedSets.has(set))
    }
  })
}

/**
 * Merges the switchable adaptation sets of the freshly loaded manifest.
 *
 * The public `manifestLoaded` event is triggered while the parsed manifest is stored
 * and before the internal stream composition reads it, so mutating the manifest object
 * is reflected in the composed streams.
 *
 * @param event dash.js manifest loaded event carrying the parsed manifest under `data`.
 */
function handleSwitchableManifestAdaptationSets(event: Record<string, unknown>): void {
  mergeSwitchableVideoManifestAdaptationSets(event.data)
}

// Register the merge before each DASH engine initialization so the manifest listener
// exists before the first manifest load of every source, regardless of bridge timing.
videojsDash.hook('beforeinitialize', (_player, mediaPlayer) => {
  mediaPlayer.on(DASH_EVENTS.manifestLoaded, handleSwitchableManifestAdaptationSets)
})

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
 * Indicates whether one resolved source is handled by the DASH source handler.
 *
 * @param source Resolved source selected for playback.
 * @returns `true` when the source must be played through dash.js.
 */
export function isDashSource(source: ResolvedVideoMediaSource): boolean {
  return source.transport === 'dash' || source.mimeType === DASH_MIME_TYPE
}

/**
 * Builds the protection entries consumed by the DASH source handler.
 *
 * The handler forwards these entries to dash.js protection data, which owns the
 * license session for DASH sources.
 *
 * @param source Resolved source selected for playback.
 * @returns Widevine protection entries, or `null` when the source is not protected.
 */
export function buildDashKeySystemOptions(
  source: ResolvedVideoMediaSource,
): VideoJsDashKeySystemOption[] | null {
  if (!source.licenseUrl) {
    return null
  }

  return [
    {
      name: WIDEVINE_KEY_SYSTEM,
      options: {
        serverURL: source.licenseUrl,
        httpRequestHeaders: source.licenseHeaders,
      },
    },
  ]
}

/**
 * Returns the dash.js MediaPlayer attached to the player by the source handler.
 *
 * @param player Video.js player currently bound to the renderer.
 * @returns dash.js MediaPlayer handle, or `null` when the source is not played through dash.js.
 */
export function getDashMediaPlayer(player: VideoJsPlayer): VideoJsDashMediaPlayerHandle | null {
  return player.dash?.mediaPlayer ?? null
}

/**
 * Indicates whether one audio track identifier was generated from a DASH period index.
 *
 * The DASH source handler rebuilds its audio track identifiers for every period, so a
 * stored identifier may point at a different language after a period or source change.
 *
 * @param trackId Audio track identifier reported by Video.js.
 * @returns `true` when the identifier was generated by the DASH source handler.
 */
export function isDashGeneratedAudioTrackId(trackId: string | null | undefined): boolean {
  return Boolean(trackId?.startsWith(DASH_AUDIO_TRACK_ID_PREFIX))
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

/** Chapter type applied to DASH advertising periods detected in the manifest. */
export const DASH_AD_CHAPTER_TYPE = 'ads'

/** dash.js events after which the manifest periods may become readable. */
const DASH_PERIOD_CHAPTER_EVENTS = [
  DASH_EVENTS.manifestLoaded,
  DASH_EVENTS.playbackMetadataLoaded,
  DASH_EVENTS.streamInitialized,
]

/** dash.js events after which Video.js track lists may have been rebuilt. */
const DASH_TRACK_LIST_EVENTS = [
  DASH_EVENTS.playbackMetadataLoaded,
  DASH_EVENTS.periodSwitchCompleted,
  'allTextTracksAdded',
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
 * @returns `true` when the DASH engine owns the chapter installation.
 */
export function installDashPeriodChapters(
  player: VideoJsPlayer,
  baseChapters: ResolvedVideoChapter[],
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
      installSkipChapterButton(player, chapters, 'ads')
    }
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
