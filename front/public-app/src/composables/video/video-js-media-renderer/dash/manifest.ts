import videojsDashModule from 'videojs-contrib-dash'

import { DASH_EVENTS } from './events'
import type {
  VideoJsDashMediaPlayerHandle,
  VideoJsPlayer,
} from '@/composables/video/video-js-media-renderer/types'

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
