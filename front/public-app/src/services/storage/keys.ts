import type { VideoJsTrackProfile } from '@/types/media'

import { DEFAULT_SUBTITLE_LANGUAGES, DEFAULT_SUBTITLE_LANGUAGE_PRIORITY } from '@/services/languages'

import type { ParameterEntry } from './types'

export { DEFAULT_SUBTITLE_LANGUAGES, DEFAULT_SUBTITLE_LANGUAGE_PRIORITY } from '@/services/languages'

/** Prefix used for all storage keys. */
export const STORE_PREFIX = 'arachnea.'
/** Prefix used for parameter storage keys. */
export const PARAM_PREFIX = 'param.'
/** Storage key for home section editing buttons visibility. */
export const HOME_SHOW_SECTION_EDITING_BUTTONS_KEY = 'home.showSectionEditingButtons'
/** Storage key for video player volume. */
export const VIDEO_PLAYER_VOLUME_KEY = 'videoPlayer.preferences.volume'
/** Storage key for video player muted state. */
export const VIDEO_PLAYER_MUTED_KEY = 'videoPlayer.preferences.muted'
/** Storage key for video player quality. */
export const VIDEO_PLAYER_QUALITY_KEY = 'videoPlayer.preferences.quality'
/** Storage key for video player audio track. */
export const VIDEO_PLAYER_AUDIO_TRACK_KEY = 'videoPlayer.preferences.audioTrack'
/** Storage key for video player text track. */
export const VIDEO_PLAYER_TEXT_TRACK_KEY = 'videoPlayer.preferences.textTrack'
/** Storage key for video player text track settings. */
export const VIDEO_PLAYER_TEXT_TRACK_SETTINGS_KEY = 'videoPlayer.preferences.textTrackSettings'
/** Storage key for the selected theme preset. */
export const THEME_KEY = 'theme.key'

/** Default ordered track profiles preserving the neutral audio/subtitle behaviour. */
export const DEFAULT_TRACK_PROFILES: readonly VideoJsTrackProfile[] = [
  { audio: 'original', subtitles: 'disabled' },
]

/** Default parameter definitions with their keys and default values. */
export const PARAMETERS_DEF: ParameterEntry[] = [
  { key: 'defaultHomeId', defaultValue: null },
  { key: 'thumbnailOrientation',          defaultValue: 'landscape'},
  { key: 'thumbnailImageFit',             defaultValue: 'contain'  },
  { key: 'thumbnailSizeMultiplier',       defaultValue: 1          },
  { key: 'collectionMode',                defaultValue: 'grid'     },
  { key: 'useTrailerAsBackground',        defaultValue: true       },
  { key: 'useCatalogBannersAsBackground', defaultValue: true       },
  { key: 'isBackgroundAnimated',          defaultValue: true       },
  { key: 'backgroundImageFit',            defaultValue: 'cover'    },
  { key: 'isEpisodeAutoplayEnabled',      defaultValue: false      },
  { key: 'videoPlayer.autoskip.previously', defaultValue: false      },
  { key: 'videoPlayer.autoskip.intro',      defaultValue: false      },
  { key: 'videoPlayer.autoskip.coming_next', defaultValue: false      },
  { key: 'videoPlayer.autoskip.outro',      defaultValue: false      },
  { key: 'videoPlayer.autoskip.ads',      defaultValue: false      },
  { key: 'videoPlayer.trackProfiles',     defaultValue: DEFAULT_TRACK_PROFILES },
  { key: 'videoPlayer.subtitleLanguagePriority', defaultValue: DEFAULT_SUBTITLE_LANGUAGE_PRIORITY },
  { key: 'videoPlayer.subtitleTranslationByLanguage', defaultValue: {} },
  { key: 'videoPlayer.subtitleLanguages',     defaultValue: DEFAULT_SUBTITLE_LANGUAGES },
  { key: 'videoPlayer.allowAudioDescription',    defaultValue: false },
  { key: 'securityMode',            defaultValue: 'safe'         },
  { key: 'language',                      defaultValue: null       },
  { key: 'theme',                         defaultValue: 'arachnea-blue' },
] as const;
