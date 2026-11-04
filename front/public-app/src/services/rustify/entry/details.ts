import { t } from '@/i18n'
import { normalizePriceAccess } from '@/services/priceAccess'
import type { EntryDetails, EntryPlayer, EntryRecommendations } from '@/types/entry'
import type { Collection } from '@/types/media'

import { call_api } from '../apiClient'
import { normalizeMediaItem } from '../media'
import {
  buildAudioLanguageLabel,
  buildSubtitleLanguageLabel,
  dedupeDisplayStrings,
  formatDurationLabel,
  formatNumberLabel,
  formatReleaseDateLabel,
  formatSeasonCount,
} from '../primitives/formatting'
import {
  firstDurationInSeconds,
  firstNonEmptyString,
  firstNumber,
  isJsonRecord,
  readBoolean,
  readFirstLink,
  readRecordList,
  readStringList,
  resolveAssetUrl,
  resolveEntryUrl,
} from '../primitives/readers'
import { normalizeEntrySeasons } from './episodes'
import { normalizeEntryPlayers, normalizeEntryTrailerResolver } from './players'

/**
 * Executes the backend entry endpoint and normalizes the response for the featured detail UI.
 *
 * @param source Backend source name that owns the entry.
 * @param entry Absolute entry URL sent to the Rust backend.
 * @param webUrl Public web URL opened by the featured action when available.
 * @returns Normalized detailed entry for the requested source.
 */
export async function getEntryDetails(
  source: string,
  entry: string,
  webUrl: string | null = null,
): Promise<EntryDetails> {
  const response = await call_api<unknown>('get_entry', { source, entry })

  if (!isJsonRecord(response)) {
    throw new Error(t('errors.unexpectedEntryResponseFormat'))
  }

  return normalizeEntryDetails(response, source, webUrl ?? entry)
}

/**
 * Executes the backend recommendations endpoint and normalizes the response for the entry detail UI.
 *
 * @param source Backend source name that owns the entry.
 * @param link Absolute recommendations URL emitted by `get_entry`.
 * @param page 1-based page number requested from the backend.
 * @returns Normalized paged recommendations payload for the requested entry.
 */
export async function getRecommendations(
  source: string,
  link: string,
  page = 1,
): Promise<EntryRecommendations> {
  const response = await call_api<unknown>('get_recommendations', { source, link, page })

  return normalizeRecommendationsPage(response, source, page)
}

/**
 * Converts one raw backend entry into the frontend detail model.
 *
 * @param entry Raw backend entry returned by `get_entry`.
 * @param source Source name used for the request.
 * @param entryUrl Absolute entry URL used for the request.
 * @returns Normalized detailed media entry.
 */
function normalizeEntryDetails(entry: unknown, source: string, entryUrl: string): EntryDetails {
  const record = isJsonRecord(entry) ? entry : {}
  const seasons = normalizeEntrySeasons(record, source)
  const players: Collection<EntryPlayer> = {
    entries: normalizeEntryPlayers(record, source),
    source,
  }
  const imagePosterUrl = resolveAssetUrl(readFirstLink(record, 'img/poster', ['img', 'poster']), source)
  const imagePortraitUrl = resolveAssetUrl(readFirstLink(record, 'img/portrait', ['img', 'portrait']), source)
  const imageLandscapeUrl = resolveAssetUrl(readFirstLink(record, 'img/landscape', ['img', 'landscape']), source)
  const imageUrl = resolveAssetUrl(
    readFirstLink(record, 'img/url', ['img', 'url']) ?? readFirstLink(record, 'img', ['img']),
    source,
  )
  const trailerUrl = firstNonEmptyString([record['video/trailer']])
  const trailerResolver = normalizeEntryTrailerResolver(record)
  const webLink = resolveEntryUrl(
    firstNonEmptyString([record['web-link'], record.webLink, record.web_url, record.webUrl]),
    source,
  )

  return {
    source,
    entryUrl: webLink ?? entryUrl,
    title: firstNonEmptyString([record.title]),
    alternativeTitleLabel: firstNonEmptyString([record['title/alt']]),
    trailerUrl,
    trailerResolver,
    players,
    description: firstNonEmptyString([record.description]),
    imagePosterUrl,
    imagePortraitUrl,
    imageLandscapeUrl,
    imageUrl,
    logoUrl: resolveAssetUrl(readFirstLink(record, 'img/logo', ['img', 'logo']), source),
    heroImageUrl: imagePortraitUrl ?? imagePosterUrl ?? imageUrl,
    yearLabel: formatNumberLabel(firstNumber(record.year)),
    releaseDateLabel: formatReleaseDateLabel(firstNonEmptyString([record['release-date']])),
    expireLabel: formatReleaseDateLabel(firstNonEmptyString([record.expire])),
    durationLabel: formatDurationLabel(firstDurationInSeconds(record.duration)),
    seasonCountLabel: formatSeasonCount(firstNumber(record['count-season'])),
    audioLanguageLabel: buildAudioLanguageLabel(record),
    subtitleLanguageLabel: buildSubtitleLanguageLabel(record),
    contentAdvisorLabel: firstNonEmptyString([record['content-advisor']]),
    themeLabels: dedupeDisplayStrings(readStringList(record.theme)),
    genreLabels: dedupeDisplayStrings(readStringList(record.genre)),
    castingLabels: dedupeDisplayStrings(readStringList(record.casting)),
    directorLabels: dedupeDisplayStrings(readStringList(record.director)),
    seasons,
    recommendations: normalizeEntryRecommendations(record.recommendations, source),
    score: firstNumber(record.rating),
    price: normalizePriceAccess(record.price),
  }
}

/**
 * Converts the optional backend recommendations section into the normalized model.
 *
 * Returns `null` when the source exposed no usable section, so the detail UI
 * hides the rail without failing the entry load.
 *
 * @param value Raw backend `recommendations` payload.
 * @param source Source name used to resolve card links and media.
 * @returns Normalized recommendations section, or `null` when absent or unusable.
 */
function normalizeEntryRecommendations(value: unknown, source: string): EntryRecommendations | null {
  if (!isJsonRecord(value)) {
    return null
  }

  const link = resolveEntryUrl(firstNonEmptyString([value.link]), source)
  const items = readRecordList(value.entries).map((entry, index) =>
    normalizeMediaItem(entry, index, source),
  )

  if (!link && items.length === 0) {
    return null
  }

  return {
    items,
    link,
    currentPage: Math.max(1, Math.trunc(firstNumber(value.current_page) ?? 1)),
    haveMore: readBoolean(value.have_more),
  }
}

/**
 * Converts one paged backend recommendations response into normalized data and pagination metadata.
 *
 * @param payload Raw backend payload returned by `get_recommendations`.
 * @param source Source name used to resolve card media.
 * @param fallbackPage Requested page number used when the backend omits page metadata.
 * @returns Normalized paged recommendations response.
 */
function normalizeRecommendationsPage(
  payload: unknown,
  source: string,
  fallbackPage: number,
): EntryRecommendations {
  if (!isJsonRecord(payload)) {
    throw new Error(t('errors.unexpectedRecommendationsResponseFormat'))
  }

  return {
    items: readRecordList(payload.entries).map((entry, index) =>
      normalizeMediaItem(entry, index, source),
    ),
    link: null,
    currentPage: Math.max(1, Math.trunc(firstNumber(payload.current_page) ?? fallbackPage)),
    haveMore: readBoolean(payload.have_more),
  }
}
