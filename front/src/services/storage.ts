// Service for managing localStorage operations
import { computed, ref, watch, type Ref } from 'vue'
import type {
  MediaCardCollectionMode,
  ThumbnailImageFit,
  ThumbnailOrientation,
  VideoJsTextTrackPreference,
  VideoJsTextTrackSettings,
  VideoJsTrackPreference,
} from '@/types/media'
import { defineStore } from 'pinia'
import {
  initHomeSectionsDB,
  getAllSections,
  saveSection,
  deleteSection,
  type HomeSectionRecord,
} from '@/services/homeSections'
import {
  initEntryBookmarksDB,
  getAllBookmarks,
  setBookmark as setBookmarkInDB,
  removeBookmark as removeBookmarkFromDB,
  buildBookmarkKey,
  BOOKMARKS_SECTION_PREFERENCE_KEY,
  type EntryBookmarkRecord,
} from '@/services/entryBookmarks'

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
  /**
   * Primary display title snapshot from `get_entry`.
   */
  title: string | null
  /**
   * Alternative title label snapshot from `get_entry`.
   */
  alternativeTitleLabel: string | null
  /**
   * Poster image URL snapshot from `get_entry`.
   */
  imagePosterUrl: string | null
  /**
   * Landscape image URL snapshot from `get_entry`.
   */
  imageLandscapeUrl: string | null
  /**
   * Description snapshot from `get_entry`.
   */
  description: string | null
  /**
   * Media type label snapshot from `get_entry`.
   */
  mediaTypeLabel: string | null
  /**
   * Display label of the currently selected season or group.
   */
  seasonLabel: string | null
  /**
   * Number of the current or last watched episode (1-based within the displayed
   * episode list), or `null` when outside an episode context.
   */
  episodeNumber: number | null
  /**
   * Whether the current episode reached its end and no episode follows it.
   */
  isFullyWatched: boolean
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
   * Last manually selected quality, or `null` for Auto.
   */
  quality: string | null
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
/** Storage key for home section editing buttons visibility. */
const HOME_SHOW_SECTION_EDITING_BUTTONS_KEY = 'home.showSectionEditingButtons'
/** Storage key for video player volume. */
const VIDEO_PLAYER_VOLUME_KEY = 'videoPlayer.preferences.volume'
/** Storage key for video player muted state. */
const VIDEO_PLAYER_MUTED_KEY = 'videoPlayer.preferences.muted'
/** Storage key for video player quality. */
const VIDEO_PLAYER_QUALITY_KEY = 'videoPlayer.preferences.quality'
/** Storage key for video player audio track. */
const VIDEO_PLAYER_AUDIO_TRACK_KEY = 'videoPlayer.preferences.audioTrack'
/** Storage key for video player text track. */
const VIDEO_PLAYER_TEXT_TRACK_KEY = 'videoPlayer.preferences.textTrack'
/** Storage key for video player text track settings. */
const VIDEO_PLAYER_TEXT_TRACK_SETTINGS_KEY = 'videoPlayer.preferences.textTrackSettings'
/** Storage key for the selected theme preset. */
const THEME_KEY = 'theme.key'
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
   * Size multiplier applied to media card thumbnails in grid and single-row modes.
   */
  thumbnailSizeMultiplier: Ref<number>
  /**
   * Layout mode applied to the result collection.
   */
  collectionMode: Ref<MediaCardCollectionMode>
  /**
   * Indicates whether the trailer should be used as the page background when available.
   */
  useTrailerAsBackground: Ref<boolean>
  /**
   * Indicates whether home/category banner and thumbnail images should be reused as the page background.
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
  /**
   * Selected theme preset key.
   */
  theme: Ref<string>
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
   * Per-section collection mode overrides.
   * Key: section preferenceKey, Value: collection mode
   */
  sectionCollectionMode: Ref<Record<string, MediaCardCollectionMode>>
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
  { key: 'thumbnailSizeMultiplier',       defaultValue: 1          },
  { key: 'collectionMode',                defaultValue: 'grid'     },
  { key: 'useTrailerAsBackground',        defaultValue: true       },
  { key: 'useCatalogBannersAsBackground', defaultValue: true       },
  { key: 'isBackgroundAnimated',          defaultValue: true       },
  { key: 'backgroundImageFit',            defaultValue: 'cover'    },
  { key: 'isEpisodeAutoplayEnabled',      defaultValue: false      },
  { key: 'language',                      defaultValue: null       },
  { key: 'theme',                         defaultValue: 'arachnea-blue' },
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

    /** Reactive cache of pinned section records, loaded from IndexedDB on init. */
    const sectionRecords = ref<HomeSectionRecord[]>([])

    /** Reactive cache of entry bookmark records, loaded from IndexedDB on init. */
    const bookmarkRecords = ref<EntryBookmarkRecord[]>([])

    /**
     * Builds a pinned section order array from the current section records cache.
     * Sections are sorted by their `order` field.
     */
    const pinnedSectionOrder = computed(() =>
      [...sectionRecords.value]
        .sort((a, b) => a.order - b.order)
        .map((record) => record.name),
    )

    /**
     * Builds a section collection mode map from the current section records cache.
     */
    const sectionCollectionMode = computed<Record<string, MediaCardCollectionMode>>(() => {
      const map: Record<string, MediaCardCollectionMode> = {}
      for (const record of sectionRecords.value) {
        map[record.name] = record.collectionMode
      }
      return map
    })

    /**
     * Builds a section thumbnail orientation map from the current section records cache.
     */
    const sectionThumbnailOrientation = computed<Record<string, ThumbnailOrientation>>(() => {
      const map: Record<string, ThumbnailOrientation> = {}
      for (const record of sectionRecords.value) {
        map[record.name] = record.thumbnailOrientation
      }
      return map
    })

    /**
     * Builds a section thumbnail image fit map from the current section records cache.
     */
    const sectionThumbnailImageFit = computed<Record<string, ThumbnailImageFit>>(() => {
      const map: Record<string, ThumbnailImageFit> = {}
      for (const record of sectionRecords.value) {
        map[record.name] = record.thumbnailImageFit
      }
      return map
    })

    /**
     * Builds an entry bookmark lookup map from the current bookmark records cache.
     */
    const entryBookmarkLookup = computed<Record<string, EntryBookmark>>(() => {
      const map: Record<string, EntryBookmark> = {}
      for (const record of bookmarkRecords.value) {
        map[record.key] = {
          groupId: record.groupId,
          selectedItemId: record.selectedItemId,
          playbackTime: record.playbackTime,
          title: record.title,
          alternativeTitleLabel: record.alternativeTitleLabel,
          imagePosterUrl: record.imagePosterUrl,
          imageLandscapeUrl: record.imageLandscapeUrl,
          description: record.description,
          mediaTypeLabel: record.mediaTypeLabel,
          seasonLabel: record.seasonLabel,
          episodeNumber: record.episodeNumber,
          isFullyWatched: record.isFullyWatched,
        }
      }
      return map
    })

    /**
     * Lists all cached bookmark records sorted by last modification, most recent first.
     */
    const entryBookmarks = computed<EntryBookmarkRecord[]>(() =>
      [...bookmarkRecords.value].sort((a, b) => b.modifiedAt - a.modifiedAt),
    )

    const homePreferences: HomePreferences = {
      pinnedSectionOrder,
      showSectionEditingButtons: ref(
        sanitizeBoolean(
          getStoreItem<unknown>(HOME_SHOW_SECTION_EDITING_BUTTONS_KEY, true),
          true,
        ),
      ),
      sectionCollectionMode,
      sectionThumbnailOrientation,
      sectionThumbnailImageFit,
    }

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
    const videoPlayerQuality = ref<string | null>(
      sanitizeQuality(
        getStoreItem<unknown>(VIDEO_PLAYER_QUALITY_KEY, null),
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
      videoPlayerQuality,
      (next) => {
        setStoreItem(VIDEO_PLAYER_QUALITY_KEY, sanitizeQuality(next))
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
     * Reloads the cached section and bookmark records from IndexedDB.
     *
     * Used to keep the in-memory cache in sync with writes performed by
     * another tab, typically when the tab becomes visible again.
     */
    async function refreshCachedRecords(): Promise<void> {
      const [sections, bookmarks] = await Promise.all([
        getAllSections(),
        getAllBookmarks(),
      ])

      sectionRecords.value = sections
      bookmarkRecords.value = bookmarks
    }

    /**
     * Initializes the IndexedDB stores and loads cached data.
     *
     * Must be called once before the store is used, typically from the app bootstrap.
     */
    async function initStorage(): Promise<void> {
      await Promise.all([
        initHomeSectionsDB(),
        initEntryBookmarksDB(),
      ])

      await refreshCachedRecords()

      // Resynchronize the cache whenever the tab becomes visible again,
      // so changes made from another tab are reflected in this one.
      document.addEventListener('visibilitychange', () => {
        if (document.visibilityState === 'visible') {
          void refreshCachedRecords()
        }
      })
      window.addEventListener('focus', () => {
        void refreshCachedRecords()
      })
    }

    /**
     * Toggles the pinned state of one section.
     *
     * Pinning creates a new record (or moves it to the end of the pinned order).
     * Unpinning deletes the record.
     *
     * @param sectionKey - Preference key of the section.
     */
    async function toggleSectionPinned(sectionKey: string): Promise<void> {
      // The local bookmarks section is always pinned and cannot be unpinned.
      if (sectionKey === BOOKMARKS_SECTION_PREFERENCE_KEY) {
        return
      }

      const existing = sectionRecords.value.find((r) => r.name === sectionKey)

      if (existing) {
        sectionRecords.value = sectionRecords.value.filter((r) => r.name !== sectionKey)
        await deleteSection(sectionKey)
        return
      }

      const nextOrder = sectionRecords.value.length
      const record: HomeSectionRecord = {
        name: sectionKey,
        order: nextOrder,
        collectionMode: 'single-row',
        thumbnailOrientation: 'portrait',
        thumbnailImageFit: 'cover',
      }
      sectionRecords.value = [...sectionRecords.value, record]
      await saveSection(record)
    }

    /**
     * Ensures a section record exists in the cache (and IndexedDB), creating it
     * with default pinned display configuration when missing.
     *
     * Used by always-pinned local sections (e.g. bookmarks) so configure and
     * move actions work before their lazy record creation completes.
     *
     * @param sectionKey - Preference key of the section.
     * @returns The existing or newly created record, or `null` on write failure.
     */
    async function ensureSectionRecord(sectionKey: string): Promise<HomeSectionRecord | null> {
      const existing = sectionRecords.value.find((r) => r.name === sectionKey)

      if (existing) {
        return existing
      }

      const record: HomeSectionRecord = {
        name: sectionKey,
        order: sectionRecords.value.length,
        collectionMode: 'single-row',
        thumbnailOrientation: 'portrait',
        thumbnailImageFit: 'cover',
      }
      sectionRecords.value = [...sectionRecords.value, record]
      await saveSection(record)
      return record
    }

    /**
     * Moves one pinned section within the ordered pinned subset.
     *
     * Materializes a missing record with default display configuration before
     * swapping, so always-pinned local sections (e.g. bookmarks) are movable
     * even on first mount before their lazy record creation completes.
     *
     * @param sectionKey - Preference key of the section to move.
     * @param direction - Direction to move the section.
     */
    async function movePinnedSection(sectionKey: string, direction: 'up' | 'down'): Promise<void> {
      await ensureSectionRecord(sectionKey)

      const ordered = [...sectionRecords.value].sort((a, b) => a.order - b.order)
      const currentIndex = ordered.findIndex((r) => r.name === sectionKey)

      if (currentIndex < 0) {
        return
      }

      const targetIndex = direction === 'up' ? currentIndex - 1 : currentIndex + 1
      if (targetIndex < 0 || targetIndex >= ordered.length) {
        return
      }

      const target = ordered[targetIndex]
      const current = ordered[currentIndex]
      if (!target || !current) {
        return
      }
      const currentOrder = current.order
      const targetOrder = target.order

      // Swap the order values
      const nextRecords = sectionRecords.value.map((r) => {
        if (r.name === sectionKey) return { ...r, order: targetOrder }
        if (r.name === target.name) return { ...r, order: currentOrder }
        return r
      })
      sectionRecords.value = nextRecords

      await Promise.all([
        saveSection({ ...current, order: targetOrder } as HomeSectionRecord),
        saveSection({ ...target, order: currentOrder } as HomeSectionRecord),
      ])
    }

    /**
     * Updates the collection mode for one section.
     *
     * If the section is not pinned yet, this is a no-op.
     *
     * @param sectionKey - Preference key of the section.
     * @param mode - New collection mode.
     */
    async function updateSectionCollectionMode(sectionKey: string, mode: MediaCardCollectionMode): Promise<void> {
      const record = await ensureSectionRecord(sectionKey)
      if (!record) {
        return
      }

      const nextRecord = { ...record, collectionMode: mode }
      sectionRecords.value = sectionRecords.value.map((r) =>
        r.name === sectionKey ? nextRecord : r,
      )
      await saveSection(nextRecord)
    }

    /**
     * Updates the thumbnail orientation for one section.
     *
     * If the section is not pinned yet, this is a no-op.
     *
     * @param sectionKey - Preference key of the section.
     * @param orientation - New thumbnail orientation.
     */
    async function updateSectionThumbnailOrientation(sectionKey: string, orientation: ThumbnailOrientation): Promise<void> {
      const record = await ensureSectionRecord(sectionKey)
      if (!record) {
        return
      }

      const nextRecord = { ...record, thumbnailOrientation: orientation }
      sectionRecords.value = sectionRecords.value.map((r) =>
        r.name === sectionKey ? nextRecord : r,
      )
      await saveSection(nextRecord)
    }

    /**
     * Updates the thumbnail image fit for one section.
     *
     * If the section is not pinned yet, this is a no-op.
     *
     * @param sectionKey - Preference key of the section.
     * @param imageFit - New thumbnail image fit.
     */
    async function updateSectionThumbnailImageFit(sectionKey: string, imageFit: ThumbnailImageFit): Promise<void> {
      const record = await ensureSectionRecord(sectionKey)
      if (!record) {
        return
      }

      const nextRecord = { ...record, thumbnailImageFit: imageFit }
      sectionRecords.value = sectionRecords.value.map((r) =>
        r.name === sectionKey ? nextRecord : r,
      )
      await saveSection(nextRecord)
    }

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
        const bookmarkKey = buildBookmarkKey(source, entry)
        return entryBookmarkLookup.value[bookmarkKey] ?? null
    }

    /**
     * Stores or replaces the bookmark for one entry details page.
     *
     * Updates the local cache and persists to IndexedDB.
     *
     * @param source Backend source identifying the entry provider.
     * @param entry Absolute entry URL passed to `get_entry`.
     * @param bookmark Bookmark payload to persist.
     */
    function setEntryBookmark(source: string, entry: string, bookmark: EntryBookmark): void {
        const key = buildBookmarkKey(source, entry)
        const sanitized = sanitizeEntryBookmark(bookmark)
        const modifiedAt = Date.now()

        // Update cache
        const nextRecords = bookmarkRecords.value.filter((r) => r.key !== key)
        nextRecords.push({
          key,
          source: source.trim(),
          entry: entry.trim(),
          modifiedAt,
          ...sanitized,
        })
        bookmarkRecords.value = nextRecords

        // Persist to IndexedDB with the same modification timestamp as the cache
        void setBookmarkInDB(source, entry, sanitized, modifiedAt)
    }

    /**
     * Removes the bookmark currently stored for one entry details page.
     *
     * Updates the local cache and persists to IndexedDB.
     *
     * @param source Backend source identifying the entry provider.
     * @param entry Absolute entry URL passed to `get_entry`.
     */
    function removeEntryBookmark(source: string, entry: string): void {
        const key = buildBookmarkKey(source, entry)

        if (!bookmarkRecords.value.some((r) => r.key === key)) {
          return
        }

        // Update cache
        bookmarkRecords.value = bookmarkRecords.value.filter((r) => r.key !== key)

        // Persist to IndexedDB
        void removeBookmarkFromDB(source, entry)
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
          quality: videoPlayerQuality.value,
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

        if (playerPreferences.quality !== null) {
          videoPlayerQuality.value = sanitizeQuality(playerPreferences.quality)
        }

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
      initStorage,
      refreshCachedRecords,
      toggleSectionPinned,
      movePinnedSection,
      updateSectionCollectionMode,
      updateSectionThumbnailOrientation,
      updateSectionThumbnailImageFit,
      getParameters,
      getHomePreferences,
      getEntryBookmark,
      setEntryBookmark,
      removeEntryBookmark,
      entryBookmarks,
      getVideoPlayerPreferences,
      setVideoPlayerPreferences,
    }
});

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
        title: null,
        alternativeTitleLabel: null,
        imagePosterUrl: null,
        imageLandscapeUrl: null,
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
function sanitizeBoolean(value: unknown, defaultValue: boolean): boolean {
    return typeof value === 'boolean' ? value : defaultValue;
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
 * Returns a sanitized 1-based episode number.
 *
 * @param value Raw storage value to sanitize.
 * @returns Positive integer episode number or `null` when invalid.
 */
function sanitizeNullableEpisodeNumber(value: unknown): number | null {
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
function sanitizeQuality(value: unknown): string | null {
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
 * @returns Track preference or `null` when no stable track identity exists.
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