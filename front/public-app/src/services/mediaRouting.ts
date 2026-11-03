import type { RouteLocationRaw } from 'vue-router'
import { encodeCategoryRouteParam, encodeEntryRouteParam, encodeLiveRoutePayload } from '@/router/routePayloads'
import type { HomeCategory } from '@/types/home'
import type { LiveSelectionTarget, MediaSelectionTarget } from '@/types/media'

/** One destination registered for source media types. */
interface MediaRouteHandler {
  /** Backend operation used by the destination view. */
  operation: 'get_entry' | 'get_category' | 'get_live'
  /** Whether artwork represents a collection of cards. */
  stackedArtwork: boolean
  /** Translation key used by the card action. */
  actionLabel: string
  /** Translation key for the featured-banner action. */
  bannerActionLabel: string
  /** Validates the target and constructs its existing Vue route. */
  buildRoute: (target: MediaSelectionTarget) => RouteLocationRaw | null
  /** Stable identity including the operation and complete target descriptor. */
  identity: (target: MediaSelectionTarget) => string | null
}

/** Current entry page, also used temporarily for audio and image content. */
const entryHandler: MediaRouteHandler = {
  operation: 'get_entry',
  stackedArtwork: false,
  actionLabel: 'entry.watch',
  bannerActionLabel: 'catalog.viewDetails',
  buildRoute: (target) => target.source?.trim() && target.entryUrl?.trim()
    ? { name: 'entry-details', params: { encodedEntry: encodeEntryRouteParam({
      source: target.source, entryUrl: target.entryUrl, title: target.title,
    }) } }
    : null,
  identity: (target) => target.source?.trim() && target.entryUrl?.trim()
    ? `get_entry:${JSON.stringify([target.source, target.entryUrl])}` : null,
}

/** Category catalogue with its complete provider request parameters. */
const collectionHandler: MediaRouteHandler = {
  operation: 'get_category',
  stackedArtwork: true,
  actionLabel: 'category.open',
  bannerActionLabel: 'category.open',
  buildRoute: (target) => validCategory(target.category)
    ? { name: 'category', params: { categoryToken: encodeCategoryRouteParam(target.category) } } : null,
  identity: (target) => validCategory(target.category)
    ? `get_category:${JSON.stringify(target.category.sources.map((source) =>
      Object.entries(source).sort(([left], [right]) => left.localeCompare(right)),
    ).map((source) => JSON.stringify(source)).sort())}` : null,
}

/** Independent live channel or event, including legacy live-list targets. */
const liveHandler: MediaRouteHandler = {
  operation: 'get_live',
  stackedArtwork: false,
  actionLabel: 'entry.watch',
  bannerActionLabel: 'entry.watch',
  buildRoute: (target) => {
    const live = liveTarget(target)
    return live ? { name: 'live-details', params: { encodedLive: encodeLiveRoutePayload({ v: 1, ...live }) } } : null
  },
  identity: (target) => {
    const live = liveTarget(target)
    return live ? `get_live:${JSON.stringify([live.source, live.channel])}` : null
  },
}

/** Single ordered mapping; exact live matching precedes the generic video family. */
const mediaRoutes: ReadonlyArray<{ exact?: string; family?: string; handler: MediaRouteHandler }> = [
  { exact: 'collection', handler: collectionHandler },
  { exact: 'video/live', handler: liveHandler },
  { family: 'video', handler: entryHandler },
  { family: 'audio', handler: entryHandler },
  { family: 'images', handler: entryHandler },
]

/**
 * Canonicalizes the scalar/list media-type contract without translating display labels.
 * @param value Source media-type scalar or list.
 * @returns Lowercase, trimmed, distinct identifiers, or an empty legacy type list.
 */
export function normalizeMediaTypes(value: unknown): string[] {
  if (value === undefined || value === null) return []
  const values = Array.isArray(value) ? value : [value]
  return [...new Set(values.map((type) => typeof type === 'string'
    ? type.trim().toLowerCase() : 'unsupported').filter(Boolean))]
}

/**
 * Resolves types without using source order to choose between incompatible pages.
 * @param values Raw or normalized media types; absent/empty types retain the legacy entry page.
 * @returns The registered handler, or null for unknown or conflicting explicit types.
 */
export function resolveMediaRouting(values: unknown): MediaRouteHandler | null {
  const types = normalizeMediaTypes(values)
  if (!types.length) return entryHandler
  const handlers = types.map((type) => mediaRoutes.find((rule) => rule.exact === type ||
    (rule.family && (type === rule.family || type.startsWith(`${rule.family}/`))))?.handler)
  if (handlers.some((handler) => !handler)) return null
  if (handlers.every((handler) => handler === handlers[0])) return handlers[0]!
  if (types.every((type) => type === 'video' || type.startsWith('video/')) && handlers.includes(liveHandler)) {
    return liveHandler
  }
  return null
}

/**
 * Builds the same validated destination for native links and application selections.
 * @param target Content with raw media types and source-owned target descriptors.
 * @returns Vue route, or null when the type or destination cannot be opened.
 */
export function mediaRoute(target: MediaSelectionTarget): RouteLocationRaw | null {
  return resolveMediaRouting(target.mediaTypeValues)?.buildRoute(target) ?? null
}

/**
 * Returns the identity shared by routing and catalogue deduplication.
 * @param target Normalized content target.
 * @returns Operation-scoped target identity, or null for an unavailable destination.
 */
export function mediaRouteIdentity(target: MediaSelectionTarget): string | null {
  return resolveMediaRouting(target.mediaTypeValues)?.identity(target) ?? null
}

/**
 * Resolves card presentation using the same registry as navigation.
 * @param target Content with raw media types.
 * @returns Collection artwork and action-label preferences.
 */
export function mediaPresentation(target: MediaSelectionTarget): Pick<MediaRouteHandler, 'stackedArtwork' | 'actionLabel' | 'bannerActionLabel'> {
  return resolveMediaRouting(target.mediaTypeValues) ?? {
    stackedArtwork: false, actionLabel: 'entry.watch', bannerActionLabel: 'catalog.viewDetails',
  }
}

/**
 * Adapts a category-strip selection to the common media routing contract.
 * @param category Complete aggregated category descriptor.
 * @returns Collection target without manufacturing an entry URL.
 */
export function categoryMediaTarget(category: HomeCategory): MediaSelectionTarget {
  return { source: null, entryUrl: null, webUrl: null, mediaTypeValues: ['collection'], category, title: category.label }
}

/**
 * Adapts an explicit live selection to the common media routing contract.
 * @param live Opaque live target and optional catalogue presentation.
 * @returns Live target without manufacturing an entry URL.
 */
export function liveMediaTarget(live: LiveSelectionTarget): MediaSelectionTarget {
  return { source: live.source, entryUrl: null, webUrl: live.webUrl ?? null, mediaTypeValues: ['video/live'], live, title: live.title }
}

/**
 * Validates category descriptors, including custom-home aggregate categories.
 * @param category Optional category built by normalization or strip selection.
 * @returns Whether each source has an identifier and a nonempty provider request.
 */
function validCategory(category: HomeCategory | undefined): category is HomeCategory {
  return Boolean(category?.sources.length && category.sources.every((source) =>
    (source.name ?? source.source)?.trim() && Object.entries(source).some(([key, value]) =>
      key !== 'name' && key !== 'source' && value.trim())))
}

/**
 * Reads the explicit live descriptor or the historical live-list opaque link.
 * @param target Content resolved to the live handler.
 * @returns Valid source/channel pair, or null for an incomplete explicit target.
 */
function liveTarget(target: MediaSelectionTarget): LiveSelectionTarget | null {
  if (target.live) return target.live.source.trim() && target.live.channel.trim() ? target.live : null
  return target.source?.trim() && target.entryUrl?.trim()
    ? { source: target.source, channel: target.entryUrl, title: target.title, webUrl: target.webUrl } : null
}
