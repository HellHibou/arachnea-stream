import type { EntryPlayer, EntryPlayerResolver, EntryPlayerStoryboard } from '@/types/entry'
import type { Collection } from '@/types/media'

import { call_api } from '../apiClient'
import { buildPlayerLabel } from '../primitives/formatting'
import {
  buildMediaId,
  firstNonEmptyString,
  firstNumber,
  isJsonRecord,
  normalizeProxyCountries,
  readBooleanFlag,
  readPath,
  readRecordList,
  resolveAssetUrl,
  resolveEntryUrl,
  toNonNegativeInteger,
  toPositiveInteger,
} from '../primitives/readers'
import type { JsonRecord } from '../types'
import { normalizeResolvedPlayerSubtitles, normalizeRustifyChapters } from './stream'

/**
 * Executes the backend players endpoint and returns the raw players payload.
 *
 * @param source Backend source name that owns the players.
 * @param link Source-specific player link or API endpoint.
 * @returns Raw players payload returned by the backend.
 */
export async function getPlayers(
  source: string,
  link: string,
): Promise<unknown> {
  return call_api<unknown>('get_players', { source, link })
}

/**
 * Converts raw backend player records into normalized embedded player entries.
 *
 * @param players Raw backend player entries.
 * @param source Source name used to resolve embed URLs.
 * @returns Normalized embedded players.
 */
export function normalizePlayers(
  players: JsonRecord[],
  source: string,
): EntryPlayer[] {
  return players
    .map((player, index) => normalizeEntryPlayer(player, index, source))
    .filter((player): player is EntryPlayer => Boolean(player))
}

/**
 * Converts backend player lists into normalized embedded player entries.
 *
 * @param entry Raw backend entry returned by `get_entry`.
 * @param source Source name used to resolve embed URLs.
 * @returns Normalized embedded players.
 */
export function normalizeEntryPlayers(entry: JsonRecord, source: string): EntryPlayer[] {
  return normalizePlayers(readRecordList(entry.players), source)
}

/**
 * Converts one raw backend get_players response into normalized player entries.
 *
 * @param payload Raw backend payload returned by get_players.
 * @param source Backend source name used to resolve player URLs.
 * @returns Normalized players.
 */
export function normalizePlayersResponse(payload: unknown, source: string): EntryPlayer[] {
  const record = isJsonRecord(payload) ? payload : {}
  return normalizePlayers(readRecordList(record.players ?? record.entries ?? record), source)
}

/**
 * Converts one raw backend player entry into the frontend detail model.
 *
 * @param entry Raw backend player entry.
 * @param index Stable fallback index used to build a unique player id.
 * @param source Source name used to resolve embed URLs.
 * @returns Normalized player, or `null` when no usable embed URL exists.
 */
function normalizeEntryPlayer(
  entry: JsonRecord,
  index: number,
  source: string,
): EntryPlayer | null {
  const embedLink = resolveEntryUrl(firstNonEmptyString([entry['embed-link']]), source)
  const directLink = resolveEntryUrl(
    firstNonEmptyString([entry['direct-link'], entry.directLink]),
    source,
  )
  const webLink = resolveEntryUrl(
    firstNonEmptyString([entry['web-link'], entry.webLink]),
    source,
  )
  const resolver = normalizeEntryPlayerResolver(entry) ?? (embedLink
    ? { kind: 'stream-resolver', targetId: embedLink }
    : null)
  const name = firstNonEmptyString([entry.name])
  const lang = firstNonEmptyString([entry.lang])
  const label = buildPlayerLabel(name, lang, index)
  const storyboard = normalizeEntryPlayerStoryboard(entry, source)
  const chapters = normalizeRustifyChapters(entry.chapters ?? entry['chapters'])
  const subtitles = normalizeResolvedPlayerSubtitles(entry.subtitles ?? entry['subtitles'])

  if (!embedLink && !directLink && !resolver) {
    return null
  }

  if (!embedLink && !directLink) {
    return {
      id: buildMediaId(index, resolver?.targetId ?? null, label, source),
      label,
      directLink: null,
      webLink,
      name,
      lang,
      resolver,
      storyboard,
      chapters,
      subtitles,
    }
  }
 
  return {
    id: buildMediaId(index, directLink ?? embedLink, label, source),
    label,
    directLink,
    webLink,
    name,
    lang,
    resolver,
    storyboard,
    chapters,
    subtitles,
  }
}

/**
 * Converts one backend player descriptor into a frontend resolver model.
 *
 * Supports two formats:
 * 1. YAML flat format (Phase 5): `resolver` and `target` as top-level player fields.
 *    - `resolver`: string value such as `"stream-resolver"` (YAML key `resolver > value`)
 *    - `target`: string URL extracted from the page (YAML key `target > value`)
 * 2. Legacy object format: `resolver` as an array of objects with `kind`, `target_id`/`targetId`,
 *    and no longer exposes a stream-kind override.
 *
 * @param entry Raw backend player entry.
 * @returns Normalized resolver descriptor, or `null` when the player is self-contained.
 */
function normalizeEntryPlayerResolver(entry: JsonRecord): EntryPlayerResolver | null {
  // Try flat format first: resolver is a direct string value, target is a direct string value.
  const flatKind = firstNonEmptyString([entry.resolver])
  const flatTarget = firstNonEmptyString([entry.target, entry['target']])

  if (flatKind && flatTarget) {
    const flatSource = firstNonEmptyString([entry.source, entry.resolverSource])
    const flatProxyCountries = normalizeProxyCountries(readPath(entry, 'proxy', 'countries'))
    const flatRewriteManifestUrls =
      readBooleanFlag(readPath(entry, 'proxy', 'rewrite_manifest_urls')) ||
      readBooleanFlag(readPath(entry, 'resolverProxy', 'rewrite_manifest_urls')) ||
      readBooleanFlag(entry.resolverProxyRewriteManifestUrls)
    return {
      kind: flatKind,
      targetId: flatTarget,
      ...(flatSource ? { source: flatSource } : {}),
      ...(flatProxyCountries.length > 0 ? { proxyCountries: flatProxyCountries } : {}),
      ...(flatRewriteManifestUrls ? { proxyRewriteManifestUrls: true } : {}),
    }
  }

  // Fall back to the legacy object format (resolver as an array of objects).
  const resolver = readRecordList(entry.resolver)[0] ?? null

  const kind = firstNonEmptyString([
    readPath(resolver, 'kind'),
    readPath(entry, 'resolverKind'),
  ])
  const targetId = firstNonEmptyString([
    readPath(resolver, 'target_id'),
    readPath(resolver, 'targetId'),
    readPath(entry, 'resolverTarget'),
    // Also check flat target from legacy top-level key
    flatTarget,
  ])
  const source = firstNonEmptyString([
    readPath(resolver, 'source'),
    readPath(entry, 'resolverSource'),
  ])
  const proxyCountries = normalizeProxyCountries(
    readPath(resolver, 'proxy', 'countries') ?? readPath(entry, 'resolverProxy', 'countries'),
  )
  const rewriteManifestUrls =
    readBooleanFlag(readPath(resolver, 'proxy', 'rewrite_manifest_urls')) ||
    readBooleanFlag(readPath(entry, 'resolverProxy', 'rewrite_manifest_urls')) ||
    readBooleanFlag(readPath(entry, 'resolverProxyRewriteManifestUrls'))

  if (!kind || !targetId) {
    return null
  }

  return {
    kind,
    targetId,
    ...(source ? { source } : {}),
    ...(proxyCountries.length > 0 ? { proxyCountries } : {}),
    ...(rewriteManifestUrls ? { proxyRewriteManifestUrls: true } : {}),
  }
}

/**
 * Converts an entry-level trailer resolver descriptor into the shared player resolver model.
 *
 * @param entry Raw backend entry returned by `get_entry`.
 * @returns Trailer resolver, or null when the entry exposes a direct trailer URL.
 */
export function normalizeEntryTrailerResolver(entry: JsonRecord): EntryPlayerResolver | null {
  const trailer = readRecordList(entry.trailer)[0] ?? null
  return trailer ? normalizeEntryPlayerResolver(trailer) : null
}

/**
 * Converts backend storyboard metadata into a Video.js sprite thumbnail configuration.
 *
 * @param entry Raw backend player entry.
 * @param source Source name used to resolve storyboard assets.
 * @returns Normalized storyboard configuration, or `null` when incomplete.
 */
function normalizeEntryPlayerStoryboard(
  entry: JsonRecord,
  source: string,
): EntryPlayerStoryboard | null {
  const url = resolveAssetUrl(
    firstNonEmptyString([
      readPath(entry, 'storyboard', 'link'),
      readPath(entry, 'storyboard', 'url'),
    ]),
    source,
  )
  const width = toPositiveInteger(firstNumber(readPath(entry, 'storyboard', 'width')))
  const height = toPositiveInteger(firstNumber(readPath(entry, 'storyboard', 'height')))
  const columns = toPositiveInteger(firstNumber(readPath(entry, 'storyboard', 'columns')))
  const rows = toPositiveInteger(firstNumber(readPath(entry, 'storyboard', 'rows'))) ?? 1
  const firstPageIndex = toNonNegativeInteger(firstNumber(readPath(entry, 'storyboard', 'first_page_index'))) ?? 0
  const indexPadding = toNonNegativeInteger(firstNumber(readPath(entry, 'storyboard', 'index_padding'))) ?? 0
  const configuredInterval = firstNumber(readPath(entry, 'storyboard', 'interval'))

  if (!url || !columns) {
    return null
  }

  const interval =
    configuredInterval !== null && Number.isFinite(configuredInterval) && configuredInterval > 0
      ? configuredInterval
      : null

  return {
    url,
    width: width ?? undefined,
    height: height ?? undefined,
    columns,
    rows,
    firstPageIndex,
    indexPadding,
    interval: interval !== null && Number.isFinite(interval) && interval > 0 ? interval : null,
  }
}
