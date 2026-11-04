import type {
  VideoJsTextTrackPreference,
  VideoJsTextTrackSettings,
  VideoJsTrackPreference,
} from '@/types/media'

import type { EntryBookmark } from './types'

/** Keys for text track settings that can be persisted. */
const TEXT_TRACK_SETTINGS_KEYS = [
  'backgroundColor',
  'backgroundOpacity',
  'color',
  'edgeStyle',
  'fontFamily',
  'fontPercent',
  'textOpacity',
  'windowColor',
  'windowOpacity',
] as const

/** Default text track preference with subtitles disabled. */
const DEFAULT_TEXT_TRACK_PREFERENCE: VideoJsTextTrackPreference = {
  id: null,
  language: null,
  label: null,
  kind: null,
  mode: 'disabled',
}

/**
 * Returns a sanitized bookmark payload safe to persist locally.
 *
 * @param value Raw storage value to sanitize.
 * @returns Sanitized bookmark payload.
 */
export function sanitizeEntryBookmark(value: unknown): EntryBookmark {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      return {
        groupId: null,
        selectedItemId: null,
        playbackTime: null,
        title: null,
        alternativeTitleLabel: null,
        imagePosterUrl: null,
        imageLandscapeUrl: null,
        imageUrl: null,
        description: null,
        mediaTypeLabel: null,
        seasonLabel: null,
        episodeNumber: null,
        isFullyWatched: false,
      };
    }

    const record = value as Record<string, unknown>;

    return {
      groupId: sanitizeNullableString(record.groupId ?? record.seasonId),
      selectedItemId: sanitizeNullableString(record.selectedItemId ?? record.episodeId),
      playbackTime: sanitizeNullablePlaybackTime(record.playbackTime),
      title: sanitizeNullableString(record.title),
      alternativeTitleLabel: sanitizeNullableString(record.alternativeTitleLabel),
      imagePosterUrl: sanitizeNullableString(record.imagePosterUrl),
      imageLandscapeUrl: sanitizeNullableString(record.imageLandscapeUrl),
      imageUrl: sanitizeNullableString(record.imageUrl),
      description: sanitizeNullableString(record.description),
      mediaTypeLabel: sanitizeNullableString(record.mediaTypeLabel),
      seasonLabel: sanitizeNullableString(record.seasonLabel),
      episodeNumber: sanitizeNullableEpisodeNumber(record.episodeNumber),
      isFullyWatched: sanitizeBoolean(record.isFullyWatched, false),
    };
}

/**
 * Returns a valid boolean or the provided default value.
 *
 * @param value Raw storage value to sanitize.
 * @param defaultValue Fallback value used when the input is invalid.
 * @returns Sanitized boolean value.
 */
export function sanitizeBoolean(value: unknown, defaultValue: boolean): boolean {
    return typeof value === 'boolean' ? value : defaultValue;
}

/**
 * Returns a sanitized nullable string.
 *
 * @param value Raw storage value to sanitize.
 * @returns Trimmed string or `null` when invalid.
 */
export function sanitizeNullableString(value: unknown): string | null {
    if (typeof value !== 'string') {
      return null;
    }

    const normalizedValue = value.trim();
    return normalizedValue || null;
}

/**
 * Returns a sanitized playback time in seconds.
 *
 * @param value Raw storage value to sanitize.
 * @returns Finite positive playback time or `null` when invalid.
 */
export function sanitizeNullablePlaybackTime(value: unknown): number | null {
    return typeof value === 'number' && Number.isFinite(value) && value > 0
      ? value
      : null;
}

/**
 * Returns a sanitized 1-based episode number.
 *
 * @param value Raw storage value to sanitize.
 * @returns Positive integer episode number or `null` when invalid.
 */
export function sanitizeNullableEpisodeNumber(value: unknown): number | null {
    return typeof value === 'number' && Number.isInteger(value) && value > 0
      ? value
      : null;
}

/**
 * Returns a volume level constrained to the range accepted by Video.js.
 *
 * @param value Raw storage value to sanitize.
 * @returns Volume level from 0 to 1.
 */
export function sanitizeVideoVolume(value: unknown): number {
    return typeof value === 'number' && Number.isFinite(value)
      ? Math.min(Math.max(value, 0), 1)
      : 1;
}

/**
 * Returns a quality preference label or `null` for automatic quality selection.
 *
 * @param value Raw storage value to sanitize.
 * @returns Trimmed manual quality label, or `null` for Auto.
 */
export function sanitizeQuality(value: unknown): string | null {
    if (typeof value !== 'string') {
      return null;
    }

    const normalizedQualityLabel = value.trim();
    return normalizedQualityLabel && normalizedQualityLabel !== 'Auto'
      ? normalizedQualityLabel
      : null;
}

/**
 * Returns a sanitized media track preference.
 *
 * @param value Raw storage value to sanitize.
 * @returns Track preference or `null` when no stable track identity exists.
 */
export function sanitizeTrackPreference(value: unknown): VideoJsTrackPreference | null {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      return null;
    }

    const record = value as Record<string, unknown>;
    const trackPreference: VideoJsTrackPreference = {
      id: sanitizeNullableString(record.id),
      language: sanitizeNullableString(record.language),
      label: sanitizeNullableString(record.label),
      kind: sanitizeNullableString(record.kind),
    };

    return trackPreference.id ||
      trackPreference.language ||
      trackPreference.label ||
      trackPreference.kind
      ? trackPreference
      : null;
}

/**
 * Returns a sanitized subtitle track preference.
 *
 * @param value Raw storage value to sanitize.
 * @returns Track preference or `null` when no stable track identity exists.
 */
export function sanitizeTextTrackPreference(value: unknown): VideoJsTextTrackPreference {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      return { ...DEFAULT_TEXT_TRACK_PREFERENCE };
    }

    const record = value as Record<string, unknown>;
    const mode = record.mode === 'showing' ? 'showing' : 'disabled';
    const trackPreference = sanitizeTrackPreference(record);

    if (mode === 'disabled') {
      return { ...DEFAULT_TEXT_TRACK_PREFERENCE };
    }

    if (!trackPreference) {
      return { ...DEFAULT_TEXT_TRACK_PREFERENCE };
    }

    return {
      ...trackPreference,
      mode,
    };
}

/**
 * Returns sanitized visual subtitle settings.
 *
 * @param value Raw storage value to sanitize.
 * @returns Subtitle settings or `null` when no setting is stored.
 */
export function sanitizeTextTrackSettings(value: unknown): VideoJsTextTrackSettings | null {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      return null;
    }

    const record = value as Record<string, unknown>;
    const settings: VideoJsTextTrackSettings = {};

    TEXT_TRACK_SETTINGS_KEYS.forEach((key) => {
      const settingValue = record[key];

      if (typeof settingValue === 'string' && settingValue.trim()) {
        settings[key] = settingValue.trim();
      } else if (typeof settingValue === 'number' && Number.isFinite(settingValue)) {
        settings[key] = settingValue;
      }
    });

    return Object.keys(settings).length ? settings : null;
}
