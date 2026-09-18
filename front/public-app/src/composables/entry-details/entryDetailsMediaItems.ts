import type { EntryEpisode, EntrySeason } from '@/types/entry'
import type { MediaItem } from '@/types/media'

/**
 * Maps one season to the media card shape reused by the existing collection component.
 *
 * @param season Season rendered in the season collection.
 * @param imagePosterUrl Poster used as a visual fallback for the season card.
 * @param imageLandscapeUrl Landscape image for the season card.
 * @param source Backend source used when the season card is selected.
 * @returns Media item compatible with the existing card collection.
 */
export function toEntrySeasonMediaItem(
  season: EntrySeason,
  imagePosterUrl: string | null,
  imageLandscapeUrl: string | null,
  source: string,
): MediaItem {
  return {
    id: season.id,
    title: season.label,
    alternativeTitleLabel: null,
    imagePosterUrl,
    imagePortraitUrl: null,
    imageLandscapeUrl,
    imageUrl: null,
    source,
    entryUrl: season.link,
    webUrl: season.link,
    mediaTypeLabel: null,
    mediaTypeValues: [],
    themeLabels: [],
    audioLabel: null,
    durationLabel: null,
    rating: null,
    overview: null,
    episodeLabel: null,
    releaseDateLabel: null,
    expireLabel: null,
    price: null,
    metaLine: null,
  }
}

/**
 * Maps one episode to the media card shape reused by the existing collection component.
 *
 * @param episode Episode rendered in the episode collection.
 * @param source Backend source used when the episode can be played.
 * @returns Media item compatible with the existing card collection.
 */
export function toEntryEpisodeMediaItem(episode: EntryEpisode, source: string): MediaItem {
  return {
    id: episode.id,
    title: episode.title?.trim() || null,
    alternativeTitleLabel: episode.alternativeTitleLabel ?? null,
    imagePosterUrl: null,
    imagePortraitUrl: null,
    imageLandscapeUrl: episode.previewUrl ?? null,
    imageUrl: null,
    source: episode.players.entries.length > 0 || episode.players.link ? source : null,
    entryUrl: episode.link ?? null,
    webUrl: episode.link ?? null,
    mediaTypeLabel: null,
    mediaTypeValues: [],
    themeLabels: [],
    audioLabel: null,
    durationLabel: episode.durationLabel,
    rating: null,
    overview: episode.description,
    episodeLabel: null,
    releaseDateLabel: episode.releaseDateLabel,
    expireLabel: episode.expireLabel,
    price: episode.price,
    metaLine: null,
  }
}
