// Service for managing localStorage operations
import { computed, ref, watch } from 'vue'
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
  type HomeSectionRecord,
} from '@/services/homeSections'
import {
  initEntryBookmarksDB,
  getAllBookmarks,
  buildBookmarkKey,
  type EntryBookmarkRecord,
} from '@/services/entryBookmarks'

import { createEntryBookmarksBridge } from './bookmarks'
import {
  HOME_SHOW_SECTION_EDITING_BUTTONS_KEY,
  PARAM_PREFIX,
  PARAMETERS_DEF,
  VIDEO_PLAYER_AUDIO_TRACK_KEY,
  VIDEO_PLAYER_MUTED_KEY,
  VIDEO_PLAYER_QUALITY_KEY,
  VIDEO_PLAYER_TEXT_TRACK_KEY,
  VIDEO_PLAYER_TEXT_TRACK_SETTINGS_KEY,
  VIDEO_PLAYER_VOLUME_KEY,
} from './keys'
import { getStoreItem, setStoreItem } from './persistence'
import {
  sanitizeBoolean,
  sanitizeNullableString,
  sanitizeQuality,
  sanitizeTextTrackPreference,
  sanitizeTextTrackSettings,
  sanitizeTrackPreference,
  sanitizeVideoVolume,
} from './sanitizers'
import { createSectionRecordsBridge } from './sectionRecords'
import type { EntryBookmark, HomePreferences, Parameters } from './types'
import { createVideoPlayerPreferencesBridge } from './videoPreferences'

export type {
  EntryBookmark,
  HomePreferences,
  Parameters,
  VideoPlayerPreferences,
} from './types'

export const useStorage = defineStore('storage', () => {

    const parameters: Parameters = {} as Parameters;
    PARAMETERS_DEF.forEach((param) => {
        const rawValue = getStoreItem(PARAM_PREFIX + param.key, param.defaultValue)
        const value = param.key === 'language' || param.key === 'defaultHomeId'
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
          imageUrl: record.imageUrl,
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

    const {
      toggleSectionPinned,
      movePinnedSection,
      updateSectionCollectionMode,
      updateSectionThumbnailOrientation,
      updateSectionThumbnailImageFit,
    } = createSectionRecordsBridge(sectionRecords)

    const { setEntryBookmark, removeEntryBookmark } = createEntryBookmarksBridge(bookmarkRecords)

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

    const { getVideoPlayerPreferences, setVideoPlayerPreferences } = createVideoPlayerPreferencesBridge({
      volume: videoPlayerVolume,
      muted: videoPlayerMuted,
      quality: videoPlayerQuality,
      audioTrack: videoPlayerAudioTrack,
      textTrack: videoPlayerTextTrack,
      textTrackSettings: videoPlayerTextTrackSettings,
    })

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
     * Save all application parameters to localStorage
     * @param parameters - Object containing parameter values to save
     */
    function saveParameters(parameters: any) {
        PARAMETERS_DEF.forEach((param) => {
          if (parameters[param.key] !== undefined)
            setStoreItem(PARAM_PREFIX + param.key, parameters[param.key]);
        });
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
