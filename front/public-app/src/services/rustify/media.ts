import { mediaRouteIdentity, normalizeMediaTypes, resolveMediaRouting } from '@/services/mediaRouting'
import { normalizePriceAccess } from '@/services/priceAccess'
import type { CatalogItem, MediaItem } from '@/types/media'

import { normalizeHomeCategory } from './home/categories'
import {
  buildMetaLine,
  dedupeDisplayStrings,
  formatDurationLabel,
  formatReleaseDateLabel,
  toDisplayMediaType,
} from './primitives/formatting'
import {
  buildMediaId,
  buildScopedId,
  firstDurationInSeconds,
  firstNonEmptyString,
  firstNumber,
  isJsonRecord,
  normalizeString,
  readFirstLink,
  readPath,
  readStringList,
  readStringParamMap,
  resolveAssetUrl,
  resolveEntryUrl,
} from './primitives/readers'
import type { JsonRecord } from './types'

/**
 * Normalizes catalog presentation and media-type targets; unavailable destinations remain non-openable.
 * @param records Backend cards in their original order.
 * @param inheritedSource Source inherited from the enclosing rail.
 * @returns Cards retaining their normalized media types and target descriptors.
 */
export function normalizeCatalogItems(records: JsonRecord[], inheritedSource: string | null): CatalogItem[] {
  return records.map((record, index) => normalizeMediaItem(record, index, inheritedSource))
}

/**
 * Derives source-owned target descriptors using the central media-type registry.
 * @param record Raw source card, including provider request parameters.
 * @param media Common presentation before target enrichment.
 * @param index Position used only for legacy category display fallbacks.
 * @param options Compatibility options for the existing live-details presentation.
 * @returns Presentation with the descriptor required by its registered destination.
 */
function normalizeMediaNavigation(
  record: JsonRecord,
  media: MediaItem,
  index: number,
  options: NormalizeMediaItemOptions,
): MediaItem {
  const handler = resolveMediaRouting(media.mediaTypeValues)
  if (!handler) {
    console.warn('Unsupported or conflicting media types', { source: media.source, mediaTypes: media.mediaTypeValues })
    return media
  }
  if (handler.operation === 'get_entry') return media
  const request = readStringParamMap(record.request)
  if (handler.operation === 'get_live') {
    const channel = firstNonEmptyString([request.channel, record.link])
    if (!media.source || !channel) return { ...media, entryUrl: null }
    const item: MediaItem = {
      ...media,
      entryUrl: options.preserveLiveEntryUrl ? channel : null,
      durationLabel: null,
      price: null,
      live: {
        source: media.source,
        channel,
        title: media.title,
        imageUrl: media.imageLandscapeUrl ?? media.imagePosterUrl ?? media.imageUrl,
        description: media.overview,
        webUrl: media.webUrl,
      },
    }
    return { ...item, id: mediaRouteIdentity(item) ?? item.id }
  }
  if (handler.operation !== 'get_category') return media
  const source = firstNonEmptyString([request.name, request.source, media.source])
  if (!source || !firstNonEmptyString([request.query_url])) return { ...media, entryUrl: null }
  const category = normalizeHomeCategory({
    ...record,
    label: media.title,
    image: media.imageUrl ?? media.imagePosterUrl ?? media.imagePortraitUrl ?? media.imageLandscapeUrl,
    source: { ...request, name: source },
  }, index, source)
  const item: MediaItem = {
    ...media,
    source,
    entryUrl: null,
    category,
    mediaTypeLabel: null,
    themeLabels: [],
    audioLabel: null,
    durationLabel: null,
    rating: null,
    episodeLabel: null,
    releaseDateLabel: null,
    expireLabel: null,
    price: null,
    metaLine: null,
  }
  return { ...item, id: mediaRouteIdentity(item) ?? item.id }
}

/**
 * Options controlling how one raw backend entry is normalized.
 */
interface NormalizeMediaItemOptions {
  /**
   * Indicates whether the media-type label should be part of the compact meta line.
   * @default true
   */
  includeMediaTypeInMetaLine?: boolean
  /** Keeps the opaque live link used internally by the existing live-details page. @default false */
  preserveLiveEntryUrl?: boolean
}

/**
 * Converts one raw backend entry into the frontend media card model.
 *
 * @param entry Raw backend search entry.
 * @param index Stable fallback index used to build a unique card id.
 * @param inheritedSource Source inherited from the enclosing section or result row.
 * @param options Optional normalization overrides.
 * @returns Normalized media item.
 */
export function normalizeMediaItem(
  entry: unknown,
  index: number,
  inheritedSource: string | null = null,
  options: NormalizeMediaItemOptions = {},
): MediaItem {
  const record = isJsonRecord(entry) ? entry : {}
  const source = firstNonEmptyString([record.source, readPath(record, 'source', 'name'), inheritedSource])
  const link = firstNonEmptyString([record.link])
  const webLink = firstNonEmptyString([record['web-link'], record.webLink])
  const rawTitle = firstNonEmptyString([record.title])
  const altTitle = firstNonEmptyString([record['title/alt']])
  const title = firstNonEmptyString([rawTitle, altTitle, record.label])
  const alternativeTitleLabel =
    rawTitle && altTitle && rawTitle.trim() !== altTitle.trim() ? altTitle : null
  const imagePosterUrl = resolveAssetUrl(
    readFirstLink(record, 'img/poster', ['img', 'poster']),
    source,
  )
  const imagePortraitUrl = resolveAssetUrl(
    readFirstLink(record, 'img/portrait', ['img', 'portrait']),
    source,
  )
  const imageLandscapeUrl = resolveAssetUrl(
    readFirstLink(record, 'img/landscape', ['img', 'landscape']),
    source,
  )
  const imageUrl = resolveAssetUrl(
    readFirstLink(record, 'img/url', ['img', 'url']) ?? readFirstLink(record, 'img', ['img']),
    source,
  )
  const entryUrl = resolveEntryUrl(link, source)
  const webUrl = resolveEntryUrl(webLink, source)
  const mediaTypeValues = normalizeMediaTypes(record['media-type'])
  const mediaTypeLabel = firstNonEmptyString(readStringList(record['media-type']).map(toDisplayMediaType))
  const themeLabels = dedupeDisplayStrings(readStringList(record.theme))
  const audioLabel = firstNonEmptyString([record.lang, record.audio, record.language])
  const durationLabel = formatDurationLabel(firstDurationInSeconds(record.duration))
  const rating = firstNumber(record.rating)
  const overview = firstNonEmptyString([record.description, record.overview])
  const episodeLabel = firstNonEmptyString([readPath(record, 'episode', 'label')])
  const releaseDateLabel = formatReleaseDateLabel(firstNonEmptyString([record['release-date']]))
  const expireLabel = formatReleaseDateLabel(firstNonEmptyString([record.expire]))
  const price = normalizePriceAccess(record.price)
  const metaLine = buildMetaLine(record, options.includeMediaTypeInMetaLine ?? true)

  const media: MediaItem = {
    request: readStringParamMap(record.request),
    id: buildMediaId(index, link, title, source),
    title,
    alternativeTitleLabel,
    imagePosterUrl,
    imagePortraitUrl,
    imageLandscapeUrl,
    imageUrl,
    source,
    entryUrl,
    webUrl,
    mediaTypeLabel,
    mediaTypeValues,
    themeLabels,
    audioLabel,
    durationLabel,
    rating,
    overview,
    episodeLabel,
    releaseDateLabel,
    expireLabel,
    price,
    metaLine,
  }
  return normalizeMediaNavigation(record, media, index, options)
}

/**
 * Deduplicates typed catalog cards while preserving their first-seen order.
 *
 * @param items Cards collected inside one merged section rail.
 * @returns Deduplicated cards with their original types.
 */
export function dedupeMediaItems<T extends CatalogItem>(items: T[]): T[] {
  const seen = new Set<string>()
  const dedupedItems: T[] = []

  items.forEach((item) => {
    const dedupeKey = buildMediaDeduplicationKey(item)
    if (seen.has(dedupeKey)) {
      return
    }

    seen.add(dedupeKey)
    dedupedItems.push(item)
  })

  return dedupedItems
}

/**
 * Builds the comparison key used to collapse duplicate media items in merged rails.
 *
 * @param item Entry, category or live card rendered inside a catalog rail.
 * @returns Stable deduplication key including the navigation role.
 */
function buildMediaDeduplicationKey(item: CatalogItem): string {
  const routeIdentity = mediaRouteIdentity(item)
  if (routeIdentity) return routeIdentity
  const normalizedSource = normalizeString(item.source ?? '')
  const normalizedEntryUrl = normalizeString(item.entryUrl ?? '')
  if (normalizedSource && normalizedEntryUrl) {
    return `entry:${normalizedSource}:${normalizedEntryUrl}`
  }

  const normalizedWebUrl = normalizeString(item.webUrl ?? '')
  if (normalizedSource && normalizedWebUrl) {
    return `web:${normalizedSource}:${normalizedWebUrl}`
  }

  const normalizedTitle = normalizeString(item.title ?? '')
  const normalizedPosterUrl = normalizeString(item.imagePosterUrl ?? item.imageUrl ?? '')
  if (normalizedSource && normalizedTitle && normalizedPosterUrl) {
    return `poster:${normalizedSource}:${normalizedTitle.toLocaleLowerCase()}:${normalizedPosterUrl}`
  }

  return `id:${item.id}`
}
