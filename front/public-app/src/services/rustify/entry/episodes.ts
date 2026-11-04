import { t } from '@/i18n'
import { normalizePriceAccess } from '@/services/priceAccess'
import type { EntryEpisode, EntryEpisodePage, EntryPlayer, EntrySeason } from '@/types/entry'
import type { Collection } from '@/types/media'

import { call_api } from '../apiClient'
import {
  formatDurationLabel,
  formatReleaseDateLabel,
} from '../primitives/formatting'
import {
  buildMediaId,
  firstDurationInSeconds,
  firstNonEmptyString,
  firstNumber,
  isJsonRecord,
  normalizeString,
  readBoolean,
  readFirstLink,
  readPath,
  readRecordList,
  resolveAssetUrl,
  resolveEntryUrl,
} from '../primitives/readers'
import type { JsonRecord } from '../types'
import { normalizePlayers } from './players'

/**
 * Executes the backend season endpoint and normalizes the response for the entry detail UI.
 *
 * @param source Backend source name that owns the season.
 * @param season Absolute season URL sent to the Rust backend.
 * @param page 1-based page number requested from the backend.
 * @param seasonName Season label used when the backend payload does not expose one per episode.
 * @returns Normalized paged season payload for the requested season.
 */
export async function getSeasonEpisodes(
  source: string,
  season: string,
  page = 1,
  seasonName: string | null = null,
): Promise<EntryEpisodePage> {
  const response = await call_api<unknown>('get_season', { source, season, page })

  return normalizeSeasonEpisodePage(response, source, seasonName, page)
}

/**
 * Converts the backend season lists into normalized season entries.
 *
 * Reads the canonical `seasons[]` field produced by `get_entry`. Each season may contain
 * embedded `episodes[]` (eager) and/or a `link` (lazy through `get_season`). When both are
 * present, the frontend prefers the embedded episodes.
 *
 * @param entry Raw backend entry returned by `get_entry`.
 * @param source Source name used to resolve the season links.
 * @returns Normalized season entries.
 */
export function normalizeEntrySeasons(entry: JsonRecord, source: string): EntrySeason[] {
  return readRecordList(entry.seasons)
    .map((season, index) => {
      const label = firstNonEmptyString([season.label])
      const normalizedLabel = label ? normalizeString(label) : null
      const link = resolveEntryUrl(firstNonEmptyString([season.link]), source)
      const rawEpisodes = readRecordList(season.episodes)
      const episodes = rawEpisodes.length > 0
        ? rawEpisodes
            .map((episode, epIndex) => normalizeEntryEpisode(episode, epIndex, source, normalizedLabel))
            .filter((episode) => !isUnavailableEpisode(episode))
        : []

      // A season without label is only acceptable when it is the only season.
      // Let the caller decide the final filtering strategy.
      return {
        id: buildMediaId(index, link, normalizedLabel ?? 'season', source),
        label: normalizedLabel,
        link,
        episodes,
      }
    })
}

/**
 * Converts eager episode lists returned directly by `get_entry`.
 *
 * @param entry Raw backend entry returned by `get_entry`.
 * @param source Source name used to resolve episode links and media.
 * @returns Normalized episodes embedded in the entry payload.
 */
export function normalizeEntryEpisodes(entry: JsonRecord, source: string): EntryEpisode[] {
  return readRecordList(entry.episode)
    .map((episode, index) => normalizeEntryEpisode(episode, index, source))
    .filter((episode) => !isUnavailableEpisode(episode))
}

/**
 * Converts one raw backend episode into the frontend episode detail model.
 *
 * @param entry Raw backend episode entry.
 * @param index Stable fallback index used to build a unique episode id.
 * @param source Source name used to resolve media assets.
 * @param fallbackSeasonName Season label used when the backend response omits it.
 * @returns Normalized episode detail.
 */
function normalizeEntryEpisode(
  entry: JsonRecord,
  index: number,
  source: string,
  fallbackSeasonName: string | null = null,
): EntryEpisode {
  const previewEntries = readRecordList(readPath(entry, 'img/preview'))
  const imagePosterUrl = readFirstLink(entry, 'img/poster', ['img', 'poster'])
  const link = resolveEntryUrl(firstNonEmptyString([entry.link]), source)
  const duration = firstDurationInSeconds(entry.duration)
  const playersLink = link
  const players: Collection<EntryPlayer> = {
    entries: normalizePlayers(readRecordList(entry.players), source),
    source,
    ...(playersLink ? { link: playersLink } : {}),
  }
  const rawTitle = firstNonEmptyString([entry.title])
  const altTitle = firstNonEmptyString([entry['title/alt']])
  const title = rawTitle || altTitle
  const alternativeTitleLabel =
    rawTitle && altTitle && rawTitle.trim() !== altTitle.trim() ? altTitle : null

  return {
    id: buildMediaId(index, link, title, source),
    link,
    players,
    seasonName: firstNonEmptyString([entry['season-name'], fallbackSeasonName]),
    title,
    alternativeTitleLabel,
    description: firstNonEmptyString([entry.description]),
    releaseDateLabel: formatReleaseDateLabel(firstNonEmptyString([entry['release-date']])),
    expireLabel: formatReleaseDateLabel(firstNonEmptyString([entry.expire])),
    durationLabel: formatDurationLabel(duration),
    price: normalizePriceAccess(entry.price),
    previewUrl: resolveAssetUrl(
      firstNonEmptyString([imagePosterUrl, ...previewEntries.map((preview) => preview.link)]),
      source,
    ),
  }
}

/**
 * Converts one paged backend season response into normalized episode data and pagination metadata.
 *
 * @param payload Raw backend payload returned by `get_season`.
 * @param source Source name used to resolve episode media.
 * @param seasonName Season label used when the backend payload does not expose one per episode.
 * @param fallbackPage Requested page number used when the backend omits page metadata.
 * @returns Normalized paged season response.
 */
function normalizeSeasonEpisodePage(
  payload: unknown,
  source: string,
  seasonName: string | null,
  fallbackPage: number,
): EntryEpisodePage {
  if (!isJsonRecord(payload)) {
    throw new Error(t('errors.unexpectedSeasonResponseFormat'))
  }

  const episodes = Array.isArray(payload.episodes) ? payload.episodes : []

  return {
    currentPage: Math.max(1, Math.trunc(firstNumber(payload.current_page) ?? fallbackPage)),
    haveMore: readBoolean(payload.have_more),
    episodes: episodes
      .map((entry, index) =>
        normalizeEntryEpisode(isJsonRecord(entry) ? entry : {}, index, source, seasonName),
      )
      .filter((episode) => !isUnavailableEpisode(episode)),
  }
}

/**
 * Builds a display title for one episode.
 *
 * @param entry Raw backend episode entry.
 * @returns Normalized episode title.
 */
function buildEpisodeTitle(entry: JsonRecord): string | null {
  return firstNonEmptyString([entry.title, entry['title/alt']])
}

/**
 * Returns whether one normalized episode should be hidden from the UI.
 *
 * @param episode Normalized episode entry.
 * @returns `true` when the episode title is empty or marked unavailable.
 */
function isUnavailableEpisode(episode: EntryEpisode): boolean {
  const title = episode.title?.trim().toLocaleLowerCase()
  return !title || title === 'titre indisponible'
}
