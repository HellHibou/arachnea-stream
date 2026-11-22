// Profile-backed personal state and device playback preferences.
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
import { isSessionContentAllowed, sessionGeneration } from '@/services/sessionEvents'
import {
  getAllSections,
  type HomeSectionRecord,
} from '@/services/homeSections'
import {
  getAllBookmarks,
  buildBookmarkKey,
  type EntryBookmarkRecord,
} from '@/services/entryBookmarks'

import { createEntryBookmarksBridge } from './bookmarks'
import { getProfileContext, isCurrentProfile, profileGeneration, type ProfileContext } from '@/services/profileContext'
import { profileDefaultHomeId } from '@/services/profileDefaultHome'
import { reportPersonalDataFailure } from '@/services/personalDataStatus'
import {
  PARAMETERS_DEF,
  VIDEO_PLAYER_MUTED_KEY,
  VIDEO_PLAYER_VOLUME_KEY,
} from './keys'
import { getStoreItem, setStoreItem } from './persistence'
import {
  sanitizeBoolean,
  sanitizeQuality,
  sanitizeTextTrackPreference,
  sanitizeTextTrackSettings,
  sanitizeTrackPreference,
  sanitizeVideoVolume,
} from './sanitizers'
import { createSectionRecordsBridge } from './sectionRecords'
import type { EntryBookmark, HomePreferences, Parameters, VideoPlayerPreferences } from './types'
import { createVideoPlayerPreferencesBridge } from './videoPreferences'
import { applyTheme, findThemePreset, themePresets } from '@/services/theme'
import { queuePlayerPreferences } from '@/services/profilePlayerPreferences'

export type {
  EntryBookmark,
  HomePreferences,
  Parameters,
  VideoPlayerPreferences,
} from './types'

export const useStorage = defineStore('storage', () => {
    let storageInitialized = false
    // The profile selection flow awaits its own refresh after publishing this event.
    window.addEventListener('arachnea-profile-context', () => { sectionRecords.value = []; bookmarkRecords.value = [] })

    const parameters: Parameters = {} as Parameters
    PARAMETERS_DEF.forEach((param) => {
      const value = structuredClone(param.defaultValue)
      ;(parameters as any)[param.key] = ref(value)
    })

    watch(profileDefaultHomeId, (id) => { parameters.defaultHomeId.value = id })

    /** Reactive cache of pinned section records, loaded from the selected profile on init. */
    const sectionRecords = ref<HomeSectionRecord[]>([])

    /** Reactive cache of entry bookmark records, loaded from the selected profile on init. */
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
      showSectionEditingButtons: ref(true),
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
    const videoPlayerQuality = ref<string | null>(null)
    const videoPlayerAudioTrack = ref<VideoJsTrackPreference | null>(null)
    const videoPlayerTextTrack = ref<VideoJsTextTrackPreference>(sanitizeTextTrackPreference(null))
    const videoPlayerTextTrackSettings = ref<VideoJsTextTrackSettings | null>(null)

    window.addEventListener('arachnea-profile-context', () => {
      PARAMETERS_DEF.forEach((param) => {
        ;(parameters as any)[param.key].value = structuredClone(param.defaultValue)
      })
      if (themePresets.length) applyTheme(findThemePreset(parameters.theme.value))
      homePreferences.showSectionEditingButtons.value = true
      videoPlayerQuality.value = null
      videoPlayerTextTrackSettings.value = null
      resetVideoPlayerTrackPreferences()
    })

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

    /** In-flight personal cache read, keyed by the profile generation it started under. */
    let cachedRefresh: { generation: number; promise: Promise<void> } | null = null
    /** Whether another refresh arrived while one was already in flight. */
    let refreshQueued = false

    /** Fetches and publishes one fresh copy of the profile-owned records. */
    async function runCachedRefresh(): Promise<void> {
      if (!isSessionContentAllowed()) return
      const generation = sessionGeneration()
      const profile = profileGeneration()
      const [sections, bookmarks] = await Promise.all([
        getAllSections(),
        getAllBookmarks(),
      ])

      if (generation !== sessionGeneration() || profile !== profileGeneration()) return
      sectionRecords.value = sections
      bookmarkRecords.value = reconcileBookmarkRecords(bookmarks)
    }

    /**
     * Reloads the selected profile’s section and bookmark records from the server.
     *
     * Concurrent calls from the same profile generation share one read; a call arriving
     * mid-flight schedules exactly one follow-up so no cross-tab change is missed.
     */
    function refreshCachedRecords(): Promise<void> {
      const generation = profileGeneration()
      const inFlight = cachedRefresh

      if (inFlight && inFlight.generation === generation) {
        refreshQueued = true
        return inFlight.promise
      }

      let promise: Promise<void>
      promise = runCachedRefresh().finally(() => {
        if (cachedRefresh?.promise !== promise) return
        cachedRefresh = null
        if (refreshQueued) {
          refreshQueued = false
          void refreshCachedRecords()
        }
      })
      cachedRefresh = { generation, promise }
      return promise
    }

    /**
     * Initializes server-backed personal caches; an unselected profile has empty collections.
     *
     * Must be called once before the store is used, typically from the app bootstrap.
     */
    async function initStorage(): Promise<void> {
      if (storageInitialized) { await refreshCachedRecords(); return }
      await refreshCachedRecords()
      storageInitialized = true

      // Resynchronize the cache whenever the tab becomes visible again,
      // so changes made from another tab are reflected in this one.
      document.addEventListener('visibilitychange', () => {
        if (document.visibilityState === 'visible') {
          void refreshCachedRecords().catch(reportPersonalDataFailure)
        }
      })
      window.addEventListener('focus', () => {
        void refreshCachedRecords().catch(reportPersonalDataFailure)
      })
    }

    const {
      toggleSectionPinned,
      movePinnedSection,
      updateSectionCollectionMode,
      updateSectionThumbnailOrientation,
      updateSectionThumbnailImageFit,
    } = createSectionRecordsBridge(sectionRecords)

    const { setEntryBookmark, removeEntryBookmark, flushEntryBookmarks, reconcileBookmarkRecords } = createEntryBookmarksBridge(bookmarkRecords)

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

    const playerPreferences = createVideoPlayerPreferencesBridge({
      volume: videoPlayerVolume,
      muted: videoPlayerMuted,
      quality: videoPlayerQuality,
      audioTrack: videoPlayerAudioTrack,
      textTrack: videoPlayerTextTrack,
      textTrackSettings: videoPlayerTextTrackSettings,
    })

    const getVideoPlayerPreferences = playerPreferences.getVideoPlayerPreferences

    /** Restores confirmed server player choices without triggering writes. @param values Streaming document. */
    function applyVideoPlayerPreferences(values: Record<string, unknown>): void {
      videoPlayerQuality.value = sanitizeQuality(values['arachnea.videoPlayer.preferences.quality'])
      videoPlayerAudioTrack.value = sanitizeTrackPreference(values['arachnea.videoPlayer.preferences.audioTrack'])
      videoPlayerTextTrack.value = sanitizeTextTrackPreference(values['arachnea.videoPlayer.preferences.textTrack'])
      videoPlayerTextTrackSettings.value = sanitizeTextTrackSettings(values['arachnea.videoPlayer.preferences.textTrackSettings'])
    }

    /** Saves user-controlled player choices under their original profile. @param value Player state. @param context Owning player context. */
    function setVideoPlayerPreferences(value: VideoPlayerPreferences, context: ProfileContext | null = getProfileContext()): void {
      if (!context || !isCurrentProfile(context)) return
      const previous = getVideoPlayerPreferences()
      playerPreferences.setVideoPlayerPreferences(value)
      const next = getVideoPlayerPreferences()
      const changed: Record<string, unknown> = {}
      for (const key of ['quality', 'audioTrack', 'textTrack', 'textTrackSettings'] as const) {
        if (JSON.stringify(previous[key]) !== JSON.stringify(next[key])) changed[`arachnea.videoPlayer.preferences.${key}`] = next[key]
      }
      if (Object.keys(changed).length) queuePlayerPreferences(changed, context)
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
      flushEntryBookmarks,
      entryBookmarks,
      applyVideoPlayerPreferences,
      getVideoPlayerPreferences,
      setVideoPlayerPreferences,
      resetVideoPlayerTrackPreferences,
      trackPreferencesRevision,
    }
});
