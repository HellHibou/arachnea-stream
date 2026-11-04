import { t, tm } from '@/i18n'

import type { JsonRecord } from '../types'
import {
  normalizeString,
  readPath,
  readStringList,
  toPositiveInteger,
} from './readers'

/**
 * Builds a display-ready player label from the backend player metadata.
 *
 * @param name Player host or backend-provided display name.
 * @param lang Optional player language label.
 * @param index Stable fallback index used when no metadata is available.
 * @returns Display-ready player label.
 */
export function buildPlayerLabel(name: string | null, lang: string | null, index: number): string {
  if (name) {
    return name
  }

  return t('player.defaultLabel', { index: String(index + 1) })
}

/**
 * Builds a compact metadata line from the backend fields available on a result.
 *
 * @param entry Raw backend entry.
 * @param includeMediaType Indicates whether the media-type label should be included.
 * @returns Short metadata line displayed in the preview panel.
 */
export function buildMetaLine(entry: JsonRecord, includeMediaType = true): string | null {
  const topicValues = [
    ...readStringList(entry.genre),
    ...readStringList(entry.theme),
  ]
  const mediaTypeValues = includeMediaType
    ? readStringList(entry['media-type']).flatMap((value) => {
        const label = toDisplayMediaType(value)
        return label ? [label] : []
      })
    : []
  const languageValues = readStringList(entry.lang)
  const values = dedupeDisplayStrings([...topicValues, ...mediaTypeValues, ...languageValues])

  return values.length > 0 ? values.join(' • ') : null
}

/**
 * Builds the displayable audio language label from the backend language-related fields.
 *
 * @param record Raw backend record.
 * @returns Combined audio language label when available.
 */
export function buildAudioLanguageLabel(record: JsonRecord): string | null {
  const values = dedupeDisplayStrings([
    ...readStringList(record.language),
    ...readStringList(readPath(record, 'lang/audio')),
  ])

  return values.length > 0 ? values.join(' • ') : null
}

/**
 * Builds the displayable subtitle language label from the backend language-related fields.
 *
 * @param record Raw backend record.
 * @returns Combined subtitle language label when available.
 */
export function buildSubtitleLanguageLabel(record: JsonRecord): string | null {
  const values = dedupeDisplayStrings(readStringList(readPath(record, 'lang/subtitles')))

  return values.length > 0 ? values.join(' • ') : null
}

/**
 * Formats a typed number for display.
 *
 * @param value Typed backend number.
 * @returns Display-ready numeric label.
 */
export function formatNumberLabel(value: number | null): string | null {
  return value === null ? null : String(value)
}

/**
 * Formats a season count extracted from the backend entry.
 *
 * @param value Typed season count.
 * @returns Display-ready season count label.
 */
export function formatSeasonCount(value: number | null): string | null {
  const count = toPositiveInteger(value)
  if (count === null) {
    return null
  }

  return count > 1
    ? t('entry.seasonCountPlural', { count: String(count) })
    : t('entry.seasonCountSingular', { count: String(count) })
}

/**
 * Formats a backend duration expressed in seconds into `h:mm:ss` or `mm:ss`.
 *
 * @param value Raw duration string returned by the backend.
 * @returns Display-ready duration label.
 */
export function formatDurationLabel(value: string | null): string | null {
  if (!value) {
    return null
  }

  const totalSeconds = parseDurationSeconds(value)

  if (totalSeconds === null) {
    return value
  }

  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const seconds = totalSeconds % 60
  const minutesLabel = String(minutes).padStart(2, '0')
  const secondsLabel = String(seconds).padStart(2, '0')

  if (hours > 0) {
    return `${hours}:${minutesLabel}:${secondsLabel}`
  }

  return `${minutesLabel}:${secondsLabel}`
}

/**
 * Parses a backend duration expressed in seconds.
 *
 * @param value Raw duration string returned by the backend.
 * @returns Parsed duration in seconds when valid.
 */
export function parseDurationSeconds(value: string | null): number | null {
  if (!value || !/^\d+$/.test(value)) {
    return null
  }

  const totalSeconds = Number.parseInt(value, 10)
  return Number.isFinite(totalSeconds) && totalSeconds >= 0 ? totalSeconds : null
}

/**
 * Formats a backend release date expressed as `YYYY-MM-DD` into `DD/MM/YYYY`.
 *
 * @param value Raw release date returned by the backend.
 * @returns Display-ready release date label.
 */
export function formatReleaseDateLabel(value: string | null): string | null {
  if (!value) {
    return null
  }

  const match = value.match(/^(\d{4})-(\d{2})-(\d{2})$/)
  if (match) {
      const [, year, month, day] = match
      return `${day}/${month}/${year}`
  }

  const match2 = value.match(/^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})$/)
  if (match2) {
      const [, year, month, day, hh, mm] = match2
      return `${day}/${month}/${year} ${hh}:${mm}`
  }

  return value;
}

/**
 * Converts backend media-type identifiers into user-facing labels.
 *
 * @param value Raw media type returned by the backend.
 * @returns Display-ready media type label.
 */
export function toDisplayMediaType(value: string): string | undefined {
  if (value.startsWith("video/other/")) {
    return value.split("video/other/")[1];
  } if (value.startsWith("video/show/other/")) {
    return value.split("video/show/other/")[1];
  } else {
    return tm('mediaTypes')[value] ?? value
  }
}

/**
 * Deduplicates display strings while preserving their original casing.
 *
 * @param values Candidate display strings.
 * @returns Deduplicated list that preserves the first display value.
 */
export function dedupeDisplayStrings(values: string[]): string[] {
  const seen = new Set<string>()
  const returnValue: string[] = []

  values.forEach((value) => {
    const normalizedValue = normalizeString(value)

    if (!normalizedValue) {
      return false
    }

    const key = normalizedValue.toLocaleLowerCase()

    if (seen.has(key)) {
      return false
    }

    seen.add(key)
    returnValue.push(normalizedValue)
    return true
  })

  return returnValue
}
