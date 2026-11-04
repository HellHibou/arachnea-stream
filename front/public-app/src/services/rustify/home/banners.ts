import type { HomeBanner, HomeBannerPlayer } from '@/types/home'

import { call_api } from '../apiClient'
import { normalizeMediaItem } from '../media'
import {
  buildMediaId,
  buildScopedId,
  firstNonEmptyString,
  isJsonRecord,
  normalizeProxyCountries,
  readBooleanFlag,
  readPath,
  readRecordList,
  resolveAssetUrl,
  resolveEntryUrl,
} from '../primitives/readers'
import type { JsonRecord } from '../types'

/**
 * Converts one raw backend banner into the frontend featured banner model.
 *
 * @param payload Raw backend banner payload.
 * @param index Stable fallback index used to build a unique banner id.
 * @returns Normalized featured banner.
 */
export function normalizeHomeBanner(
  payload: unknown,
  index: number,
  inheritedSource: string | null = null,
): HomeBanner {
  const record = isJsonRecord(payload) ? payload : {}
  const source = firstNonEmptyString([record.source, readPath(record, 'source', 'name'), inheritedSource])
  const entryUrl = resolveEntryUrl(
    firstNonEmptyString([record.entry_url, record.entryUrl, record.link]),
    source,
  )
  const webUrl = resolveEntryUrl(
    firstNonEmptyString([record.web_url, record.webUrl, record['web-link'], record.link]),
    source,
  )
  const imageUrl = resolveAssetUrl(
    firstNonEmptyString([record.image_url, record.imageUrl, record.image]),
    source,
  )
  const logoUrl = resolveAssetUrl(
    firstNonEmptyString([record.logo_url, record.logoUrl, record.logo]),
    source,
  )
  const videoUrl = resolveAssetUrl(
    firstNonEmptyString([record.video_url, record.videoUrl, record.video]),
    source,
  )
  const title = firstNonEmptyString([record.title])

  const player = extractBannerPlayer(record)

  return {
    id:
      buildScopedId(firstNonEmptyString([record.id, record.key]), source) ??
      buildMediaId(index, entryUrl ?? imageUrl, title, source),
    title,
    subtitle: firstNonEmptyString([record.subtitle]),
    description: firstNonEmptyString([record.description]),
    imageUrl,
    logoUrl,
    videoUrl,
    player,
    source,
    target: normalizeMediaItem({
      ...record,
      source,
      link: entryUrl ?? webUrl,
      'web-link': webUrl,
      'img/url': { link: imageUrl },
    }, index, source),
    entryUrl: entryUrl ?? webUrl,
    webUrl: webUrl ?? entryUrl,
  }
}

/**
 * Extracts the first player resolver from a raw banner record.
 *
 * @param record Raw banner record returned by the backend.
 * @returns Player descriptor or null when no player is available.
 */
function extractBannerPlayer(record: Record<string, unknown>): HomeBannerPlayer | null {
  if (!Array.isArray(record.players)) {
    return null
  }

  for (const player of record.players) {
    const resolver = isJsonRecord(player) ? (player as Record<string, unknown>).resolver : null
    const kind = isJsonRecord(resolver) ? firstNonEmptyString([(resolver as Record<string, unknown>).kind]) : null
    const targetId = isJsonRecord(resolver) ? firstNonEmptyString([(resolver as Record<string, unknown>).target_id]) : null
    const source = isJsonRecord(resolver) ? firstNonEmptyString([(resolver as Record<string, unknown>).source]) : null
    const proxyCountries = isJsonRecord(resolver)
      ? normalizeProxyCountries(readPath(resolver, 'proxy', 'countries'))
      : []
    const rewriteManifestUrls = isJsonRecord(resolver)
      ? readBooleanFlag(readPath(resolver, 'proxy', 'rewrite_manifest_urls'))
      : false

    if (kind && targetId) {
      return {
        kind,
        targetId,
        ...(source ? { source } : {}),
        ...(proxyCountries.length > 0 ? { proxyCountries } : {}),
        ...(rewriteManifestUrls ? { proxyRewriteManifestUrls: true } : {}),
      }
    }
  }

  return null
}

/**
 * Converts one raw backend `get_banners` response into a normalized banner array.
 *
 * @param payload Raw backend payload returned by `get_banners`.
 * @param source Backend source name used to resolve banner asset URLs.
 * @param startIndex Stable offset used to build banner ids when prepending or appending.
 * @returns Normalized home banners.
 */
export function normalizeBannersResponse(
  payload: unknown,
  source: string | null,
  startIndex = 0,
): HomeBanner[] {
  const record = isJsonRecord(payload) ? payload : {}
  const banners = readBannerList(record.banners ?? record.entries ?? record)
    .map((banner, index) => normalizeHomeBanner(banner, startIndex + index, source))
    .filter(
      (banner) =>
        banner.entryUrl &&
        (banner.imageUrl || banner.title || banner.videoUrl || banner.player),
    )

  return banners.sort((left, right) =>
    Number(isBannerVideoPlayable(right)) - Number(isBannerVideoPlayable(left)),
  )
}

/**
 * Returns whether a banner carries a directly playable video or a stream resolver.
 *
 * @param banner Normalized featured banner.
 * @returns True when the banner can render video in the hero carousel.
 */
function isBannerVideoPlayable(banner: HomeBanner): boolean {
  return Boolean(banner.videoUrl || banner.player)
}

/**
 * Executes the backend banners endpoint and returns the raw banners payload.
 *
 * @param source Backend source name that owns the banners.
 * @param link Source-specific banner link or API endpoint.
 * @returns Raw banners payload returned by the backend.
 */
export async function getBanners(
  source: string,
  link: string,
): Promise<unknown> {
  return call_api<unknown>('get_banners', { source, link })
}

/**
 * Reads home banners while tolerating both arrays of banner objects and one indexed object payload.
 *
 * Some scraper outputs can serialize repeated banner fields into one object whose properties are
 * aligned arrays. When that happens, the frontend must rebuild one record per index instead of
 * treating the whole object as a single banner.
 *
 * @param value Unknown backend banner node.
 * @returns Banner records ready for normalization.
 */
export function readBannerList(value: unknown): JsonRecord[] {
  if (Array.isArray(value)) {
    return value.flatMap((entry) => {
      if (isJsonRecord(entry)) {
        return expandIndexedBannerRecords([entry])
      }

      if (Array.isArray(entry)) {
        const fragments = entry.filter(isJsonRecord)
        return fragments.length > 0 ? [mergeBannerFragments(fragments)] : []
      }

      return []
    })
  }

  return expandIndexedBannerRecords(readRecordList(value))
}

/**
 * Expands one banner payload when the backend aligned several banner fields in one indexed object.
 *
 * @param records Candidate banner records extracted from the backend node.
 * @returns One or more banner records ready for normalization.
 */
function expandIndexedBannerRecords(records: JsonRecord[]): JsonRecord[] {
  if (records.length !== 1) {
    return records
  }

  const candidate = records[0]
  if (!candidate) {
    return []
  }

  const entries = Object.entries(candidate)
  const maxFieldLength = entries.reduce(
    (maxLength, [, fieldValue]) => Math.max(maxLength, readIndexedFieldValues(fieldValue).length),
    0,
  )

  if (maxFieldLength <= 1) {
    return records
  }

  return Array.from({ length: maxFieldLength }, (_, index) => {
    const record: JsonRecord = {}

    entries.forEach(([key, fieldValue]) => {
      const indexedValue = readIndexedFieldValues(fieldValue)[index]
      if (indexedValue !== undefined && indexedValue !== null) {
        record[key] = indexedValue
      }
    })

    return record
  }).filter((record) => Object.keys(record).length > 0)
}

/**
 * Rebuilds one banner record from several partial fragments emitted for the same backend item.
 *
 * Some Rust-serialized banner items are emitted as an array of partial objects when one field
 * contains more values than the others. The frontend only needs one consolidated record.
 *
 * @param fragments Partial records that all belong to the same banner item.
 * @returns Consolidated banner record.
 */
function mergeBannerFragments(fragments: JsonRecord[]): JsonRecord {
  const record: JsonRecord = {}

  fragments.forEach((fragment) => {
    Object.entries(fragment).forEach(([key, value]) => {
      if (!(key in record) && value !== null && value !== undefined) {
        record[key] = value
      }
    })
  })

  return record
}

/**
 * Returns the aligned values attached to one potentially indexed backend field.
 *
 * @param value Unknown backend field value.
 * @returns Raw values that can be consumed by one indexed banner record.
 */
function readIndexedFieldValues(value: unknown): unknown[] {
  if (Array.isArray(value)) {
    return value
  }

  if (isJsonRecord(value) && Array.isArray(value._)) {
    return value._
  }

  return value === null || value === undefined ? [] : [value]
}
