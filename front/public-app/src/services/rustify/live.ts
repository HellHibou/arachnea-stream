import { t } from '@/i18n'
import type { EntryPlayer } from '@/types/entry'
import type { LiveSelectionTarget, MediaItem } from '@/types/media'

import { call_api } from './apiClient'
import { normalizeEntryPlayers } from './entry/players'
import { normalizeMediaItem } from './media'
import { isJsonRecord } from './primitives/readers'

/**
 * Executes the backend live listing endpoint and normalizes the response for the live details UI.
 *
 * @returns Normalized live media cards consumable by the shared collection components.
 */
export async function listLiveMediaItems(): Promise<MediaItem[]> {
  const response = await call_api<unknown[]>('list_lives', {})

  if (!Array.isArray(response)) {
    throw new Error(t('errors.unexpectedLiveListResponseFormat'))
  }

  // Every live catalog entry is a live channel, so its media-type label is redundant in the list.
  return response.map((entry, index) =>
    normalizeMediaItem(entry, index, null, { includeMediaTypeInMetaLine: false, preserveLiveEntryUrl: true }),
  )
}

/**
 * Executes the backend live endpoint and normalizes the returned embedded players.
 *
 * @param source Backend source name that owns the live channel.
 * @param channel Source-specific opaque live channel or event identifier.
 * @returns Normalized players available for the selected live channel.
 */
export async function getLivePlayers(source: string, channel: string): Promise<EntryPlayer[]> {
  const response = await call_api<unknown>('get_live', { source, channel })

  if (!isJsonRecord(response)) {
    throw new Error(t('errors.unexpectedLiveResponseFormat'))
  }

  return normalizeEntryPlayers(response, source)
}

/**
 * Builds the live-details presentation for a target that need not appear in list_lives.
 * @param target Explicit source, live identifier and optional catalog presentation.
 * @returns A media presentation consumed by the existing live player page.
 */
export function liveTargetMediaItem(target: LiveSelectionTarget): MediaItem {
  return {
    ...normalizeMediaItem({
      source: target.source,
      link: target.channel,
      title: target.title ?? t('live.title'),
      description: target.description,
      'web-link': target.webUrl,
      'img/url': { link: target.imageUrl },
      'media-type': 'video/live',
    }, 0, target.source),
    id: `live:${JSON.stringify([target.source, target.channel])}`,
    entryUrl: target.channel,
  }
}
