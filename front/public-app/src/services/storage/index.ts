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
  DEFAULT_SUBTITLE_LANGUAGES,
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
  createDefaultSubtitleLanguagePriority,
  sanitizeBoolean,
  sanitizeNullableString,
  sanitizeQuality,
  sanitizeSubtitleLanguagePriority,
  sanitizeSubtitleLanguages,
  sanitizeSubtitleTranslationByLanguage,
  sanitizeTextTrackPreference,
  sanitizeTextTrackSettings,
  sanitizeTrackPreference,
  sanitizeTrackProfiles,
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

/** Parameter key for the interface language. */
const LANGUAGE_PARAMETER_KEY = 'language'
/** Parameter key for the subtitle language priority list. */
const SUBTITLE_LANGUAGE_PRIORITY_PARAMETER_KEY = 'videoPlayer.subtitleLanguagePriority'
/** Parameter key for per-language subtitle translation authorization. */
const SUBTITLE_TRANSLATION_BY_LANGUAGE_PARAMETER_KEY = 'videoPlayer.subtitleTranslationByLanguage'

/**
 * Checks whether two language priority lists are identical.
 *
 * @param left First language list.
 * @param right Second language list.
 * @returns True when both lists contain the same codes in the same order.
 */
function areLanguageListsEqual(left: readonly string[], right: readonly string[]): boolean {
    return left.length === right.length
      && left.every((language, index) => language === right[index]);
}

/**
 * Checks whether two translation authorization maps are identical.
 *
 * @param left First authorization map.
 * @param right Second authorization map.
 * @returns True when both maps contain the same enabled codes.
 */
function areTranslationLanguageMapsEqual(
  left: Readonly<Record<string, boolean>>,
  right: Readonly<Record<string, boolean>>,
): boolean {
  const leftKeys = Object.keys(left)
  const rightKeys = Object.keys(right)
  return leftKeys.length === rightKeys.length
    && leftKeys.every((key) => left[key] === true && right[key] === true)
}

/**
 * Sanitizes one persisted parameter value.
 *
 * @param key Parameter key.
 * @param value Raw stored value.
 * @param defaultSubtitleLanguagePriority Interface-derived subtitle language default.
 * @param subtitleLanguagePriority Current subtitle language priority.
 * @returns Sanitized parameter value.
 */
function sanitizeParameterValue(
  key: string,
  value: unknown,
  defaultSubtitleLanguagePriority: string[],
  subtitleLanguagePriority: readonly string[] = defaultSubtitleLanguagePriority,
): unknown {
    switch (key) {
      case LANGUAGE_PARAMETER_KEY:
      case 'defaultHomeId':
        return sanitizeNullableString(value);
      case 'videoPlayer.trackProfiles':
        return sanitizeTrackProfiles(value);
      case SUBTITLE_LANGUAGE_PRIORITY_PARAMETER_KEY:
        return sanitizeSubtitleLanguagePriority(value, defaultSubtitleLanguagePriority);
      case SUBTITLE_TRANSLATION_BY_LANGUAGE_PARAMETER_KEY:
        return sanitizeSubtitleTranslationByLanguage(value, subtitleLanguagePriority);
      case 'videoPlayer.subtitleLanguages':
        return sanitizeSubtitleLanguages(value, DEFAULT_SUBTITLE_LANGUAGES);
      case 'videoPlayer.allowAudioDescription':
        return sanitizeBoolean(value, false);
      default:
        return value;
    }
}

export const useStorage = defineStore('storage', () => {

    const storedLanguage = sanitizeNullableString(
      getStoreItem(PARAM_PREFIX + LANGUAGE_PARAMETER_KEY, null),
    )
    const defaultSubtitleLanguagePriority = createDefaultSubtitleLanguagePriority(storedLanguage)
    const initialSubtitleLanguagePriority = sanitizeSubtitleLanguagePriority(
      getStoreItem(
        PARAM_PREFIX + SUBTITLE_LANGUAGE_PRIORITY_PARAMETER_KEY,
        getStoreItem(PARAM_PREFIX + 'videoPlayer.subtitleLanguages', null),
      ),
      defaultSubtitleLanguagePriority,
    )
    let hasCustomSubtitleLanguagePriority = !areLanguageListsEqual(
      initialSubtitleLanguagePriority,
      defaultSubtitleLanguagePriority,
    )

    const parameters: Parameters = {} as Parameters;
    PARAMETERS_DEF.forEach((param) => {
        const defaultValue = param.key === SUBTITLE_LANGUAGE_PRIORITY_PARAMETER_KEY
          ? initialSubtitleLanguagePriority
          : param.defaultValue
        const rawValue = getStoreItem(PARAM_PREFIX + param.key, defaultValue)
        const value = sanitizeParameterValue(
          param.key,
          rawValue,
          defaultSubtitleLanguagePriority,
          (parameters as Partial<Parameters>)[SUBTITLE_LANGUAGE_PRIORITY_PARAMETER_KEY]?.value
            ?? initialSubtitleLanguagePriority,
        )
        ;(parameters as any) [param.key] = ref(value);
        watch((parameters as any) [param.key], (new_val) => {
            setStoreItem(
              PARAM_PREFIX + param.key,
              sanitizeParameterValue(
                param.key,
                new_val,
                defaultSubtitleLanguagePriority,
                parameters[SUBTITLE_LANGUAGE_PRIORITY_PARAMETER_KEY].value,
              ),
            );
        })
    });

    watch(parameters.language, (nextLanguage) => {
      if (hasCustomSubtitleLanguagePriority) {
        return
      }

      parameters[SUBTITLE_LANGUAGE_PRIORITY_PARAMETER_KEY].value =
        createDefaultSubtitleLanguagePriority(nextLanguage)
    })

    watch(
      parameters[SUBTITLE_LANGUAGE_PRIORITY_PARAMETER_KEY],
      (nextPriority) => {
        const currentDefault = createDefaultSubtitleLanguagePriority(parameters.language.value)
        hasCustomSubtitleLanguagePriority = !areLanguageListsEqual(
          sanitizeSubtitleLanguagePriority(nextPriority, currentDefault),
          currentDefault,
        )

        const translationByLanguage = parameters[SUBTITLE_TRANSLATION_BY_LANGUAGE_PARAMETER_KEY].value
        const sanitizedTranslationByLanguage = sanitizeSubtitleTranslationByLanguage(
          translationByLanguage,
          nextPriority,
        )
        if (!areTranslationLanguageMapsEqual(
          translationByLanguage,
          sanitizedTranslationByLanguage,
        )) {
          parameters[SUBTITLE_TRANSLATION_BY_LANGUAGE_PARAMETER_KEY].value =
            sanitizedTranslationByLanguage
        }
      },
      { deep: true },
    )

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

    /** Invalidates track selections held by mounted players after a settings edit. */
    const trackPreferencesRevision = ref(0)

    /** Clears manual track choices so current profiles take effect on every player. */
    function resetVideoPlayerTrackPreferences(): void {
      videoPlayerAudioTrack.value = null
      videoPlayerTextTrack.value = sanitizeTextTrackPreference(null)
      trackPreferencesRevision.value += 1
    }

    watch(
      [
        parameters['videoPlayer.trackProfiles'],
        parameters['videoPlayer.subtitleLanguagePriority'],
        parameters['videoPlayer.subtitleTranslationByLanguage'],
        parameters['videoPlayer.allowAudioDescription'],
      ],
      resetVideoPlayerTrackPreferences,
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
      resetVideoPlayerTrackPreferences,
      trackPreferencesRevision,
    }
});
