import type {
  EntryPlayer,
  EntryStreamSource,
  EntryPlayerChapter,
  EntryPlayerStoryboard,
  EntryResolvedPlayerStream,
  GetStreamResponse,
  ResolvedPlayerSubtitle,
} from '@/types/entry'

import { call_api } from '../apiClient'
import {
  firstNonEmptyString,
  firstNumber,
  isJsonRecord,
  readOptionalBoolean,
  readPath,
  readStringMap,
  toNonNegativeInteger,
  toPositiveInteger,
} from '../primitives/readers'

/**
 * Resolves one backend player into a directly playable stream when the source needs a custom
 * playback handshake such as DRM token retrieval.
 *
 * @param player Normalized player descriptor selected in the UI.
 * @returns Resolved stream payload, or `null` when the player does not need extra resolution.
 */
export async function getStream(
  player: EntryPlayer,
): Promise<GetStreamResponse | null> {
  if (!player.resolver) {
    return null
  }

  const response = await call_api<unknown>('get_stream', {
    resolver: player.resolver.kind,
    target: player.resolver.targetId,
    source: player.resolver.source,
    proxy_countries: player.resolver.proxyCountries,
    proxy_rewrite_manifest_urls: player.resolver.proxyRewriteManifestUrls,
  })

  return normalizeGetStreamResponse(response)
}

/**
 * Reads the structured stream contract shared by direct players and resolved players.
 * @param payload Player or resolution response containing streams and shared metadata.
 * @returns A normalized collection, or null when no valid source remains.
 */
export function normalizePlayerStream(payload: unknown): EntryResolvedPlayerStream | null {
  if (!isJsonRecord(payload) || !Array.isArray(payload.streams)) return null
  const streams: EntryStreamSource[] = []
  for (const item of payload.streams) {
    if (!isJsonRecord(item)) continue
    const url = firstNonEmptyString([item.url])
    const type = item.type
    if (!url || !['hls', 'dash', 'mp4', 'webm', 'ogg'].includes(type as string)) continue
    streams.push({
      url, type: type as EntryStreamSource['type'],
      quality: firstNonEmptyString([item.quality]), lang: firstNonEmptyString([item.lang])?.toLocaleLowerCase() ?? null,
      licenseUrl: firstNonEmptyString([item.license_url]),
      licenseHeaders: readStringMap(item.license_headers),
    })
  }
  if (streams.length === 0) return null
  return {
    streams, imageTitleLink: firstNonEmptyString([readPath(payload, 'image/title', 'link')]),
    storyboardVttUrl: firstNonEmptyString([payload.storyboard_vtt_url]),
    storyboard: normalizeRustifyStoryboard(payload.storyboard),
    chapters: normalizeRustifyChapters(payload.chapters),
    subtitles: normalizeResolvedPlayerSubtitles(payload.subtitles),
  }
}

/**
 * Normalizes the optional subtitle tracks returned by the backend.
 *
 * @param value Backend subtitle track array.
 * @returns Normalized subtitle tracks, or an empty array when absent or invalid.
 */
export function normalizeResolvedPlayerSubtitles(value: unknown): ResolvedPlayerSubtitle[] {
  if (!Array.isArray(value)) {
    return []
  }

  const subtitles: ResolvedPlayerSubtitle[] = []

  for (const item of value) {
    if (!isJsonRecord(item)) {
      continue
    }

    const link = typeof item.link === 'string' ? item.link.trim() : ''
    if (!link) {
      continue
    }

    const lang = typeof item.lang === 'string' ? item.lang.trim() : ''
    const label = typeof item.label === 'string' ? item.label.trim() : ''
    const autoTranslate =
      readOptionalBoolean(item.auto_translate) ??
      readOptionalBoolean(item.autoTranslate)

    subtitles.push({
      link,
      ...(lang ? { lang } : {}),
      ...(label ? { label } : {}),
      ...(autoTranslate !== null ? { autoTranslate } : {}),
    })
  }

  return subtitles
}

/**
 * Normalizes backend chapters, discarding non-finite or empty time ranges.
 *
 * @param value Backend chapters array.
 * @returns Normalized chapters array, or `null` when absent or empty.
 */
export function normalizeRustifyChapters(value: unknown): EntryPlayerChapter[] | null {
  if (!Array.isArray(value)) {
    return null
  }

  const chapters: EntryPlayerChapter[] = []

  for (const item of value) {
    if (!isJsonRecord(item)) {
      continue
    }

    const start = item.start
    const end = item.end
    const title = item.title ?? null
    const chapterType = item.type

    if (
      typeof start !== 'number' ||
      typeof end !== 'number' ||
      !Number.isFinite(start) ||
      !Number.isFinite(end) ||
      end <= start ||
      typeof chapterType !== 'string'
    ) {
      continue
    }

    chapters.push({
      start,
      end,
      title: typeof title === 'string' ? title.trim() : '',
      type: chapterType,
    })
  }

  return chapters.length > 0 ? chapters : null
}
function normalizeRustifyStoryboard(value: unknown): EntryPlayerStoryboard | null {
  if (!isJsonRecord(value)) {
    return null
  }

  const url = firstNonEmptyString([value.url, value.link])
  const width = toPositiveInteger(firstNumber(value.width))
  const height = toPositiveInteger(firstNumber(value.height))
  const columns = toPositiveInteger(firstNumber(value.columns))
  const rows = toPositiveInteger(firstNumber(value.rows)) ?? 1
  const firstPageIndex = toNonNegativeInteger(firstNumber(value.first_page_index ?? value.firstPageIndex)) ?? 0
  const indexPadding = toNonNegativeInteger(firstNumber(value.index_padding ?? value.indexPadding)) ?? 0
  const interval = firstNumber(value.interval)

  if (!url || !columns) {
    return null
  }

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

/**
 * Normalizes one `get_stream` backend response into the frontend union type.
 *
 * The response can be either a structured stream payload (`streams[]` and metadata)
 * or an iframe fallback (`embed-link`). The two branches are mutually exclusive.
 *
 * @param payload Raw backend payload returned by `get_stream`.
 * @returns Normalized union response, or `null` when the payload is unusable.
 */
function normalizeGetStreamResponse(payload: unknown): GetStreamResponse | null {
  if (!isJsonRecord(payload)) {
    return null
  }

  const embedLink = firstNonEmptyString([payload['embed-link']])
  if (embedLink) return 'streams' in payload ? null : { embedLink }
  return normalizePlayerStream(payload)
}
