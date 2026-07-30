// Service for managing localStorage operations
import { ref, watch, type Ref } from 'vue'
import type {
  MediaCardCollectionMode,
  ThumbnailImageFit,
  ThumbnailOrientation,
  VideoJsTextTrackPreference,
  VideoJsTextTrackSettings,
  VideoJsTrackPreference,
} from '@/types/media'
import { defineStore } from 'pinia'

/**
 * Configuration entry for a storable parameter.
 */
interface ParameterEntry {
  /** The parameter key used for storage. */
  key: string
  /** The default value for the parameter. */
  defaultValue: any
}

/**
 * Bookmark persisted for one entry details view.
 */
export interface EntryBookmark {
  /**
   * Selected group identifier to restore when reopening the entry.
   */
  groupId: string | null
  /**
   * Selected playable item identifier to restore when reopening the entry.
   */
  selectedItemId: string | null
  /**
   * Last known playback position in seconds for Video.js media.
   */
  playbackTime: number | null
}

/**
 * Video.js preferences persisted across player instances.
 */
export interface VideoPlayerPreferences {
  /**
   * Last selected volume level from 0 to 1.
   */
  volume: number
  /**
   * Indicates whether the player was muted.
   */
  muted: boolean
  /**
   * Last manually selected quality label, or `null` for Auto.
   */
  qualityLabel: string | null
  /**
   * Last selected audio track, or `null` when unavailable.
   */
  audioTrack: VideoJsTrackPreference | null
  /**
   * Last selected subtitle track, or disabled subtitle state.
   */
  textTrack: VideoJsTextTrackPreference
  /**
   * Last selected visual subtitle settings.
   */
  textTrackSettings: VideoJsTextTrackSettings | null
}

/** Prefix used for all storage keys. */
const STORE_PREFIX = "arachnea."
/** Prefix used for parameter storage keys. */
const PARAM_PREFIX = "param."
/** Storage key for home pinned section order. */
const HOME_PINNED_SECTION_ORDER_KEY = 'home.pinnedSectionOrder'
/** Storage key for home section editing buttons visibility. */
const HOME_SHOW_SECTION_EDITING_BUTTONS_KEY = 'home.showSectionEditingButtons'
/** Storage key for home favorite collection mode. */
const HOME_FAVORITE_COLLECTION_MODE_KEY = 'home.favoriteCollectionMode'
/** Storage key for home section thumbnail orientation. */
const HOME_SECTION_THUMBNAIL_ORIENTATION_KEY = 'home.sectionThumbnailOrientation'
/** Storage key for home section thumbnail image fit. */
const HOME_SECTION_THUMBNAIL_IMAGE_FIT_KEY = 'home.sectionThumbnailImageFit'
/** Storage key for entry bookmarks. */
const ENTRY_BOOKMARKS_KEY = 'entry.bookmarks'
/** Storage key for video player volume. */
const VIDEO_PLAYER_VOLUME_KEY = 'videoPlayer.preferences.volume'
/** Storage key for video player muted state. */
const VIDEO_PLAYER_MUTED_KEY = 'videoPlayer.preferences.muted'
/** Storage key for video player quality label. */
const VIDEO_PLAYER_QUALITY_LABEL_KEY = 'videoPlayer.preferences.qualityLabel'
/** Storage key for video player audio track. */
const VIDEO_PLAYER_AUDIO_TRACK_KEY = 'videoPlayer.preferences.audioTrack'
/** Storage key for video player text track. */
const VIDEO_PLAYER_TEXT_TRACK_KEY = 'videoPlayer.preferences.textTrack'
/** Storage key for video player text track settings. */
const VIDEO_PLAYER_TEXT_TRACK_SETTINGS_KEY = 'videoPlayer.preferences.textTrackSettings'
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
 * Props accepted by the search display parameters component.
 */
export interface Parameters {
  /**
   * Thumbnail orientation applied to search results.
   */
  thumbnailOrientation: Ref<ThumbnailOrientation>
  /**
   * Poster fit mode applied to search results.
   */
  thumbnailImageFit: Ref<ThumbnailImageFit>
  /**
   * Layout mode applied to the result collection.
   */
  collectionMode: Ref<MediaCardCollectionMode>
  /**
   * Indicates whether the trailer should be used as the page background when available.
   */
  useTrailerAsBackground: Ref<boolean>
  /**
   * Indicates whether home/category banner images should be reused as the page background.
   */
  useCatalogBannersAsBackground: Ref<boolean>
  /**
   * Indicates whether the page background should animate.
   */
  isBackgroundAnimated: Ref<boolean>
  /**
   * Controls whether the background image should be fully visible or cropped.
   */
  backgroundImageFit: Ref<ThumbnailImageFit>
  /**
   * Indicates whether the integrated player should automatically start the next episode.
   */
  isEpisodeAutoplayEnabled: Ref<boolean>
  /**
   * Preferred interface language, or `null` to follow the browser language.
   */
  language: Ref<string | null>
}

/**
 * Home catalog preferences persisted locally.
 */
export interface HomePreferences {
  /**
   * Ordered list of pinned home section keys.
   */
  pinnedSectionOrder: Ref<string[]>
  /**
   * Indicates whether home editing buttons should remain visible.
   */
  showSectionEditingButtons: Ref<boolean>
  /**
   * Layout mode applied to pinned home sections.
   */
  favoriteCollectionMode: Ref<MediaCardCollectionMode>
  /**
   * Per-section thumbnail orientation overrides.
   * Key: section preferenceKey, Value: orientation
   */
  sectionThumbnailOrientation: Ref<Record<string, ThumbnailOrientation>>
  /**
   * Per-section thumbnail image fit overrides.
   * Key: section preferenceKey, Value: fit mode
   */
  sectionThumbnailImageFit: Ref<Record<string, ThumbnailImageFit>>
}

/** Default parameter definitions with their keys and default values. */
const PARAMETERS_DEF: ParameterEntry[] = [
  { key: 'thumbnailOrientation',          defaultValue: 'landscape'},
  { key: 'thumbnailImageFit',             defaultValue: 'contain'  },
  { key: 'collectionMode',                defaultValue: 'grid'     },
  { key: 'useTrailerAsBackground',        defaultValue: true       },
  { key: 'useCatalogBannersAsBackground', defaultValue: true       },
  { key: 'isBackgroundAnimated',          defaultValue: true       },
  { key: 'backgroundImageFit',            defaultValue: 'cover'    },
  { key: 'isEpisodeAutoplayEnabled',      defaultValue: false      },
  { key: 'language',                      defaultValue: null       },
] as const;

export const useStorage = defineStore('storage', () => {

    const parameters: Parameters = {} as Parameters;
    PARAMETERS_DEF.forEach((param) => {
        const rawValue = getStoreItem(PARAM_PREFIX + param.key, param.defaultValue)
        const value = param.key === 'language'
          ? sanitizeNullableString(rawValue)
          : rawValue
        ;(parameters as any) [param.key] = ref(value);
        watch((parameters as any) [param.key], (new_val, old_val) => {
            setStoreItem(PARAM_PREFIX + param.key, new_val);
        })
    });

     const homePreferences: HomePreferences = {
       pinnedSectionOrder: ref(
         sanitizeStringArray(getStoreItem<unknown>(HOME_PINNED_SECTION_ORDER_KEY, [])),
       ),
       showSectionEditingButtons: ref(
         sanitizeBoolean(
           getStoreItem<unknown>(HOME_SHOW_SECTION_EDITING_BUTTONS_KEY, true),
           true,
         ),
       ),
       favoriteCollectionMode: ref(
         sanitizeMediaCardCollectionMode(
           getStoreItem<unknown>(HOME_FAVORITE_COLLECTION_MODE_KEY, 'single-row'),
           'single-row',
         ),
       ),
       sectionThumbnailOrientation: ref(
         sanitizeSectionThumbnailOrientation(
           getStoreItem<unknown>(HOME_SECTION_THUMBNAIL_ORIENTATION_KEY, {}),
         ),
       ),
       sectionThumbnailImageFit: ref(
         sanitizeSectionThumbnailImageFit(
           getStoreItem<unknown>(HOME_SECTION_THUMBNAIL_IMAGE_FIT_KEY, {}),
         ),
       ),
     }

    const entryBookmarks = ref<Record<string, EntryBookmark>>(
      sanitizeEntryBookmarks(getStoreItem<unknown>(ENTRY_BOOKMARKS_KEY, {})),
    )
    const videoPlayerVolume = ref<number>(
      sanitizeVideoVolume(
        getStoreItem<unknown>(VIDEO_PLAYER_VOLUME_KEY, null),
      ),
    )
    const videoPlayerMuted = ref<boolean>(
      sanitizeBoolean(
        getStoreItem<unknown>(VIDEO_PLAYER_MUTED_KEY, null),
        false,
      ),
    )
    const videoPlayerQualityLabel = ref<string | null>(
      sanitizeQualityPreference(
        getStoreItem<unknown>(VIDEO_PLAYER_QUALITY_LABEL_KEY, null),
      ),
    )
    const videoPlayerAudioTrack = ref<VideoJsTrackPreference | null>(
      sanitizeTrackPreference(
        getStoreItem<unknown>(VIDEO_PLAYER_AUDIO_TRACK_KEY, null),
      ),
    )
    const videoPlayerTextTrack = ref<VideoJsTextTrackPreference>(
      sanitizeTextTrackPreference(
        getStoreItem<unknown>(VIDEO_PLAYER_TEXT_TRACK_KEY, null),
      ),
    )
    const videoPlayerTextTrackSettings = ref<VideoJsTextTrackSettings | null>(
      sanitizeTextTrackSettings(
        getStoreItem<unknown>(VIDEO_PLAYER_TEXT_TRACK_SETTINGS_KEY, null),
      ),
    )

    watch(
      homePreferences.pinnedSectionOrder,
      (nextPinnedSectionOrder) => {
        const sanitizedPinnedSectionOrder = sanitizeStringArray(nextPinnedSectionOrder)

        if (!areStringArraysEqual(nextPinnedSectionOrder, sanitizedPinnedSectionOrder)) {
          homePreferences.pinnedSectionOrder.value = sanitizedPinnedSectionOrder
          return
        }

        setStoreItem(HOME_PINNED_SECTION_ORDER_KEY, sanitizedPinnedSectionOrder)
      },
      { deep: true },
    )

    watch(
      homePreferences.showSectionEditingButtons,
      (nextShowSectionEditingButtons) => {
        const sanitizedShowSectionEditingButtons = sanitizeBoolean(
          nextShowSectionEditingButtons,
          true,
        )

        if (nextShowSectionEditingButtons !== sanitizedShowSectionEditingButtons) {
          homePreferences.showSectionEditingButtons.value = sanitizedShowSectionEditingButtons
          return
        }

        setStoreItem(
          HOME_SHOW_SECTION_EDITING_BUTTONS_KEY,
          sanitizedShowSectionEditingButtons,
        )
      },
    )

     watch(
       homePreferences.favoriteCollectionMode,
       (nextFavoriteCollectionMode) => {
         const sanitizedFavoriteCollectionMode = sanitizeMediaCardCollectionMode(
           nextFavoriteCollectionMode,
           'single-row',
         )

         if (nextFavoriteCollectionMode !== sanitizedFavoriteCollectionMode) {
           homePreferences.favoriteCollectionMode.value = sanitizedFavoriteCollectionMode
           return
         }

         setStoreItem(HOME_FAVORITE_COLLECTION_MODE_KEY, sanitizedFavoriteCollectionMode)
       },
     )

     watch(
       homePreferences.sectionThumbnailOrientation,
       (nextSectionThumbnailOrientation) => {
         setStoreItem('home.sectionThumbnailOrientation', nextSectionThumbnailOrientation)
       },
       { deep: true },
     )

     watch(
       homePreferences.sectionThumbnailImageFit,
       (nextSectionThumbnailImageFit) => {
         setStoreItem('home.sectionThumbnailImageFit', nextSectionThumbnailImageFit)
       },
       { deep: true },
     )

    watch(
      entryBookmarks,
      (nextEntryBookmarks) => {
        setStoreItem(ENTRY_BOOKMARKS_KEY, sanitizeEntryBookmarks(nextEntryBookmarks))
      },
      { deep: true },
    )

    watch(
      videoPlayerVolume,
      (next) => {
        setStoreItem(VIDEO_PLAYER_VOLUME_KEY, sanitizeVideoVolume(next))
      },
    )

    watch(
      videoPlayerMuted,
      (next) => {
        setStoreItem(VIDEO_PLAYER_MUTED_KEY, sanitizeBoolean(next, false))
      },
    )

    watch(
      videoPlayerQualityLabel,
      (next) => {
        setStoreItem(VIDEO_PLAYER_QUALITY_LABEL_KEY, sanitizeQualityPreference(next))
      },
    )

    watch(
      videoPlayerAudioTrack,
      (next) => {
        setStoreItem(VIDEO_PLAYER_AUDIO_TRACK_KEY, sanitizeTrackPreference(next))
      },
      { deep: true },
    )

    watch(
      videoPlayerTextTrack,
      (next) => {
        setStoreItem(VIDEO_PLAYER_TEXT_TRACK_KEY, sanitizeTextTrackPreference(next))
      },
      { deep: true },
    )

    watch(
      videoPlayerTextTrackSettings,
      (next) => {
        setStoreItem(VIDEO_PLAYER_TEXT_TRACK_SETTINGS_KEY, sanitizeTextTrackSettings(next))
      },
      { deep: true },
    )

    /**
     * Save data to localStorage
     * @param key - Storage key
     * @param data - Data to store (will be JSON.stringified)
     */
    function setStoreItem<T>(key: string, data: T): void {
        try {
          localStorage.setItem(STORE_PREFIX + key, JSON.stringify(data));
        } catch (error) {
          console.error(`Failed to save item ${key} to localStorage:`, data, error);
        }
    }

    /**
     * Retrieve data from localStorage with default value
     * @param key - Storage key
     * @param defaultValue - Default value to return if not found/invalid
     * @returns Parsed data or default value
     */
    function getStoreItem<T>(key: string, defaultValue: T | null = null): T | null {
        try {
          const item = localStorage.getItem(STORE_PREFIX + key);
          return item ? (JSON.parse(item) as T) : defaultValue;
        } catch (error) {
          console.error(`Failed to retrieve item ${key} from localStorage:`, error);
          localStorage.removeItem(STORE_PREFIX + key);
          return defaultValue;
        }
    }

    /**
     * Remove item from localStorage
     * @param key - Storage key to remove
     */
    function removeStoreItem(key: string): void {
        try {
          localStorage.removeItem(STORE_PREFIX + key);
        } catch (error) {
          console.error(`Failed to remove item ${key} from localStorage:`, error);
        }
    }

    /**
     * Clear all localStorage data
     */
    function clearStore(): void {
        try {
          localStorage.clear();
        } catch (error) {
          console.error('Failed to clear localStorage:', error);
        }
    }

    /**
     * Get all application parameters
     * @returns Object containing all parameter values
     */
    function getParameters(): Parameters {
        return parameters;
    }

    /**
     * Get all home catalog preferences.
     *
     * @returns Object containing home ordering and display preferences.
     */
    function getHomePreferences(): HomePreferences {
        return homePreferences;
    }

    /**
     * Returns the bookmark currently stored for one entry details page.
     *
     * @param source Backend source identifying the entry provider.
     * @param entry Absolute entry URL passed to `get_entry`.
     * @returns Stored bookmark or `null` when none exists.
     */
    function getEntryBookmark(source: string, entry: string): EntryBookmark | null {
        const bookmarkKey = buildEntryBookmarkKey(source, entry)
        return entryBookmarks.value[bookmarkKey] ?? null
    }

    /**
     * Stores or replaces the bookmark for one entry details page.
     *
     * @param source Backend source identifying the entry provider.
     * @param entry Absolute entry URL passed to `get_entry`.
     * @param bookmark Bookmark payload to persist.
     */
    function setEntryBookmark(source: string, entry: string, bookmark: EntryBookmark): void {
        const bookmarkKey = buildEntryBookmarkKey(source, entry)
        const sanitizedBookmark = sanitizeEntryBookmark(bookmark)

        entryBookmarks.value = {
          ...entryBookmarks.value,
          [bookmarkKey]: sanitizedBookmark,
        }
    }

    /**
     * Removes the bookmark currently stored for one entry details page.
     *
     * @param source Backend source identifying the entry provider.
     * @param entry Absolute entry URL passed to `get_entry`.
     */
    function removeEntryBookmark(source: string, entry: string): void {
        const bookmarkKey = buildEntryBookmarkKey(source, entry)

        if (!(bookmarkKey in entryBookmarks.value)) {
          return
        }

        const nextEntryBookmarks = { ...entryBookmarks.value }
        delete nextEntryBookmarks[bookmarkKey]
        entryBookmarks.value = nextEntryBookmarks
    }

    /**
     * Returns the persisted Video.js preferences.
     *
     * @returns Sanitized player preferences used to restore new player instances.
     */
    function getVideoPlayerPreferences(): VideoPlayerPreferences {
        return {
          volume: videoPlayerVolume.value,
          muted: videoPlayerMuted.value,
          qualityLabel: videoPlayerQualityLabel.value,
          audioTrack: videoPlayerAudioTrack.value,
          textTrack: videoPlayerTextTrack.value,
          textTrackSettings: videoPlayerTextTrackSettings.value,
        }
    }

    /**
     * Stores the durable part of one Video.js state snapshot.
     *
     * @param playerPreferences Latest durable player preferences.
     */
    function setVideoPlayerPreferences(playerPreferences: VideoPlayerPreferences): void {
        videoPlayerVolume.value = sanitizeVideoVolume(playerPreferences.volume)
        videoPlayerMuted.value = sanitizeBoolean(playerPreferences.muted, false)
        videoPlayerQualityLabel.value = sanitizeQualityPreference(playerPreferences.qualityLabel)
        videoPlayerAudioTrack.value = sanitizeTrackPreference(playerPreferences.audioTrack)
        videoPlayerTextTrack.value = sanitizeTextTrackPreference(playerPreferences.textTrack)
        videoPlayerTextTrackSettings.value = sanitizeTextTrackSettings(playerPreferences.textTrackSettings)
    }

    /**
     * Save all application parameters to localStorage
     * @param parameters - Object containing parameter values to save
     */
    function saveParameters(parameters: any) {
        PARAMETERS_DEF.forEach((param) => {
          if (parameters[param.key] !== undefined)
            setStoreItem(PARAM_PREFIX + param.key, parameters[param.key]);
        });
    }

    /**
     * Check if localStorage is available
     * @returns true if localStorage is available
     */
    function isStoreAvailable(): boolean {
        try {
          const testKey = '__storage_test__';
          localStorage.setItem(testKey, 'test');
          localStorage.removeItem(testKey);
          return true;
        } catch (e) {
          return false;
        }
    }

    return {
      getParameters,
      getHomePreferences,
      getEntryBookmark,
      setEntryBookmark,
      removeEntryBookmark,
      getVideoPlayerPreferences,
      setVideoPlayerPreferences,
    }
});

/**
 * Returns a deduplicated array of non-empty strings while preserving order.
 *
 * @param value Raw storage value to sanitize.
 * @returns Sanitized array safe to persist in local storage.
 */
function sanitizeStringArray(value: unknown): string[] {
    if (!Array.isArray(value)) {
      return [];
    }

    const seen = new Set<string>();
    const sanitizedValues: string[] = [];

    value.forEach((entry) => {
      if (typeof entry !== 'string') {
        return;
      }

      const normalizedEntry = entry.trim();
      if (!normalizedEntry || seen.has(normalizedEntry)) {
        return;
      }

      seen.add(normalizedEntry);
      sanitizedValues.push(normalizedEntry);
    });

    return sanitizedValues;
}

/**
 * Returns whether two string arrays contain the same values in the same order.
 *
 * @param left First array to compare.
 * @param right Second array to compare.
 * @returns `true` when both arrays are identical.
 */
function areStringArraysEqual(left: string[], right: string[]): boolean {
    if (left.length !== right.length) {
      return false;
    }

    return left.every((value, index) => value === right[index]);
}

/**
 * Returns a valid boolean or the provided default value.
 *
 * @param value Raw storage value to sanitize.
 * @param defaultValue Fallback value used when the input is invalid.
 * @returns Sanitized boolean value.
 */
function sanitizeBoolean(value: unknown, defaultValue: boolean): boolean {
    return typeof value === 'boolean' ? value : defaultValue;
}

/**
 * Returns a valid collection layout or the provided default value.
 *
 * @param value Raw storage value to sanitize.
 * @param defaultValue Fallback value used when the input is invalid.
 * @returns Sanitized media card collection mode.
 */
function sanitizeMediaCardCollectionMode(
    value: unknown,
    defaultValue: MediaCardCollectionMode,
): MediaCardCollectionMode {
    return value === 'grid' || value === 'single-row' || value === 'list'
      ? value
      : defaultValue;
}

/**
 * Returns a sanitized record of section thumbnail orientations.
 *
 * @param value Raw storage value to sanitize.
 * @returns Sanitized record mapping section keys to thumbnail orientations.
 */
function sanitizeSectionThumbnailOrientation(value: unknown): Record<string, ThumbnailOrientation> {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
        return {};
    }

    const sanitized: Record<string, ThumbnailOrientation> = {};
    for (const [key, val] of Object.entries(value)) {
        if (typeof key === 'string' && (val === 'portrait' || val === 'landscape')) {
            sanitized[key] = val;
        }
    }

    return sanitized;
}

/**
 * Returns a sanitized record of section thumbnail image fits.
 *
 * @param value Raw storage value to sanitize.
 * @returns Sanitized record mapping section keys to thumbnail image fits.
 */
function sanitizeSectionThumbnailImageFit(value: unknown): Record<string, ThumbnailImageFit> {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
        return {};
    }

    const sanitized: Record<string, ThumbnailImageFit> = {};
    for (const [key, val] of Object.entries(value)) {
        if (typeof key === 'string' && (val === 'cover' || val === 'contain')) {
            sanitized[key] = val;
        }
    }

    return sanitized;
}

/**
 * Builds the storage key used for one entry bookmark.
 *
 * @param source Backend source identifying the entry provider.
 * @param entry Absolute entry URL passed to `get_entry`.
 * @returns Stable bookmark key used in local storage.
 */
function buildEntryBookmarkKey(source: string, entry: string): string {
    return JSON.stringify([source.trim(), entry.trim()]);
}

/**
 * Returns a sanitized bookmark payload safe to persist locally.
 *
 * @param value Raw storage value to sanitize.
 * @returns Sanitized bookmark payload.
 */
function sanitizeEntryBookmark(value: unknown): EntryBookmark {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      return {
        groupId: null,
        selectedItemId: null,
        playbackTime: null,
      };
    }

    const record = value as Record<string, unknown>;

    return {
      groupId: sanitizeNullableString(record.groupId ?? record.seasonId),
      selectedItemId: sanitizeNullableString(record.selectedItemId ?? record.episodeId),
      playbackTime: sanitizeNullablePlaybackTime(record.playbackTime),
    };
}

/**
 * Returns a sanitized bookmark record map.
 *
 * @param value Raw storage value to sanitize.
 * @returns Sanitized map of entry bookmarks.
 */
function sanitizeEntryBookmarks(value: unknown): Record<string, EntryBookmark> {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      return {};
    }

    const sanitizedBookmarks: Record<string, EntryBookmark> = {};

    for (const [key, bookmarkValue] of Object.entries(value)) {
      if (typeof key !== 'string' || !key.trim()) {
        continue;
      }

      sanitizedBookmarks[key] = sanitizeEntryBookmark(bookmarkValue);
    }

    return sanitizedBookmarks;
}

/**
 * Returns a sanitized nullable string.
 *
 * @param value Raw storage value to sanitize.
 * @returns Trimmed string or `null` when invalid.
 */
function sanitizeNullableString(value: unknown): string | null {
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
function sanitizeNullablePlaybackTime(value: unknown): number | null {
    return typeof value === 'number' && Number.isFinite(value) && value > 0
      ? value
      : null;
}

/**
 * Returns a volume level constrained to the range accepted by Video.js.
 *
 * @param value Raw storage value to sanitize.
 * @returns Volume level from 0 to 1.
 */
function sanitizeVideoVolume(value: unknown): number {
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
function sanitizeQualityPreference(value: unknown): string | null {
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
function sanitizeTrackPreference(value: unknown): VideoJsTrackPreference | null {
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
 * @returns Subtitle preference, defaulting to disabled when invalid.
 */
function sanitizeTextTrackPreference(value: unknown): VideoJsTextTrackPreference {
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
function sanitizeTextTrackSettings(value: unknown): VideoJsTextTrackSettings | null {
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
