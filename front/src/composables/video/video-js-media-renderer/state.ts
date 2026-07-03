import { REMAINING_TIME_CLASS } from '@/composables/video/video-js-media-renderer/constants'
import {
  getSelectedQualityLabel,
  normalizeQualityPreferenceLabel,
} from '@/composables/video/video-js-media-renderer/quality'
import type {
  VideoJsAudioTrackHandle,
  VideoJsAudioTrackListHandle,
  VideoJsCurrentSource,
  VideoJsPlayer,
  VideoJsTextTrackHandle,
  VideoJsTextTrackListHandle,
  VideoJsTextTrackSettingsHandle,
} from '@/composables/video/video-js-media-renderer/types'
import { t } from '@/i18n'
import type {
  VideoJsPlayerState,
  VideoJsTextTrackPreference,
  VideoJsTextTrackSettings,
  VideoJsTrackPreference,
} from '@/types/media'

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

/** Set of text track kinds that should be displayed. */
const DISPLAY_TEXT_TRACK_KINDS = new Set(['captions', 'subtitles'])

/** Default text track preference representing disabled state. */
const DISABLED_TEXT_TRACK_PREFERENCE: VideoJsTextTrackPreference = {
  id: null,
  language: null,
  label: null,
  kind: null,
  mode: 'disabled',
}

/**
 * Normalizes a track field value to a string or null.
 *
 * @param value - Value to normalize.
 * @returns Trimmed string or null if not a valid string.
 */
function normalizeTrackField(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null
}

/**
 * Creates a track preference object from a track handle.
 *
 * @param track - Audio or text track handle.
 * @returns Track preference object with normalized fields.
 */
function createTrackPreference(
  track: VideoJsAudioTrackHandle | VideoJsTextTrackHandle,
): VideoJsTrackPreference {
  return {
    id: normalizeTrackField(track.id),
    language: normalizeTrackField(track.language),
    label: normalizeTrackField(track.label),
    kind: normalizeTrackField(track.kind),
  }
}

/**
 * Checks if a text track should be displayed.
 *
 * @param track - Text track to check.
 * @returns True when the track kind is in the displayable set.
 */
function isDisplayTextTrack(track: VideoJsTextTrackHandle): boolean {
  return Boolean(track.kind && DISPLAY_TEXT_TRACK_KINDS.has(track.kind))
}

/**
 * Gets the text track settings handle from the player.
 *
 * @param player - Video.js player instance.
 * @returns Text track settings handle or null.
 */
function getTextTrackSettings(player: VideoJsPlayer): VideoJsTextTrackSettingsHandle | null {
  const settings = player.textTrackSettings ??
    (player.getChild?.('textTrackSettings') as VideoJsTextTrackSettingsHandle | null | undefined)

  return settings ?? null
}

/**
 * Gets the audio track list handle from the player.
 *
 * @param player - Video.js player instance.
 * @returns Audio track list handle or null.
 */
function getAudioTracks(player: VideoJsPlayer): VideoJsAudioTrackListHandle | null {
  return (player.audioTracks?.() as unknown as VideoJsAudioTrackListHandle | undefined) ?? null
}

/**
 * Gets the text track list handle from the player.
 *
 * @param player - Video.js player instance.
 * @returns Text track list handle or null.
 */
function getTextTracks(player: VideoJsPlayer): VideoJsTextTrackListHandle | null {
  return (player.textTracks?.() as unknown as VideoJsTextTrackListHandle | undefined) ?? null
}

/**
 * Captures the currently selected audio track from the player.
 *
 * @param player - Video.js player instance.
 * @returns Audio track preference or null if none selected.
 */
function captureSelectedAudioTrack(player: VideoJsPlayer): VideoJsTrackPreference | null {
  const audioTracks = getAudioTracks(player)

  if (!audioTracks) {
    return null
  }

  for (let index = 0; index < audioTracks.length; index += 1) {
    const track = audioTracks[index]

    if (track?.enabled) {
      return createTrackPreference(track)
    }
  }

  return null
}

/**
 * Captures the currently selected text track from the player.
 *
 * @param player - Video.js player instance.
 * @returns Text track preference or disabled preference if none selected.
 */
function captureSelectedTextTrack(player: VideoJsPlayer): VideoJsTextTrackPreference {
  const textTracks = getTextTracks(player)

  if (!textTracks) {
    return DISABLED_TEXT_TRACK_PREFERENCE
  }

  for (let index = 0; index < textTracks.length; index += 1) {
    const track = textTracks[index]

    if (track?.mode === 'showing' && isDisplayTextTrack(track)) {
      return {
        ...createTrackPreference(track),
        mode: 'showing',
      }
    }
  }

  return DISABLED_TEXT_TRACK_PREFERENCE
}

/**
 * Captures the current text track settings from the player.
 *
 * @param player - Video.js player instance.
 * @returns Text track settings object or null if no settings available.
 */
function captureTextTrackSettings(player: VideoJsPlayer): VideoJsTextTrackSettings | null {
  const values = getTextTrackSettings(player)?.getValues?.()

  if (!values) {
    return null
  }

  const settings: VideoJsTextTrackSettings = {}

  TEXT_TRACK_SETTINGS_KEYS.forEach((key) => {
    const value = values[key]

    if (typeof value === 'string' && value.trim()) {
      settings[key] = value.trim()
    } else if (typeof value === 'number' && Number.isFinite(value)) {
      settings[key] = value
    }
  })

  return Object.keys(settings).length ? settings : null
}

/**
 * Calculates a match score between a track and a preference.
 * Higher scores indicate better matches.
 *
 * @param track - Track to match.
 * @param preference - Preference to match against.
 * @returns Match score (0-100).
 */
function getTrackMatchScore(
  track: VideoJsAudioTrackHandle | VideoJsTextTrackHandle,
  preference: VideoJsTrackPreference,
): number {
  const id = normalizeTrackField(track.id)
  const language = normalizeTrackField(track.language)
  const label = normalizeTrackField(track.label)
  const kind = normalizeTrackField(track.kind)

  if (preference.id && id === preference.id) {
    return 100
  }

  let score = 0

  if (preference.language && language === preference.language) {
    score += 30
  }

  if (preference.kind && kind === preference.kind) {
    score += 20
  }

  if (preference.label && label === preference.label) {
    score += 10
  }

  return score
}

/**
 * Finds the best matching track for a given preference.
 *
 * @param tracks - Array-like collection of tracks.
 * @param preference - Preference to match.
 * @param filter - Optional filter function for tracks.
 * @returns Best matching track or null.
 */
function findBestMatchingTrack<T extends VideoJsAudioTrackHandle | VideoJsTextTrackHandle>(
  tracks: ArrayLike<T>,
  preference: VideoJsTrackPreference,
  filter: (track: T) => boolean = () => true,
): T | null {
  let matchingTrack: T | null = null
  let matchingScore = 0

  for (let index = 0; index < tracks.length; index += 1) {
    const track = tracks[index]

    if (!track || !filter(track)) {
      continue
    }

    const score = getTrackMatchScore(track, preference)

    if (score > matchingScore) {
      matchingTrack = track
      matchingScore = score
    }
  }

  return matchingTrack
}

/**
 * Restores the audio track selection based on preference.
 *
 * @param player - Video.js player instance.
 * @param preference - Audio track preference to restore.
 */
function restoreAudioTrack(player: VideoJsPlayer, preference: VideoJsTrackPreference | null) {
  if (!preference) {
    return
  }

  const audioTracks = getAudioTracks(player)

  if (!audioTracks) {
    return
  }

  const matchingTrack = findBestMatchingTrack(audioTracks, preference)

  if (!matchingTrack) {
    return
  }

  for (let index = 0; index < audioTracks.length; index += 1) {
    const track = audioTracks[index]

    if (track) {
      track.enabled = track === matchingTrack
    }
  }
}

/**
 * Restores the text track selection based on preference.
 *
 * @param player - Video.js player instance.
 * @param preference - Text track preference to restore.
 */
function restoreTextTrack(
  player: VideoJsPlayer,
  preference: VideoJsTextTrackPreference | null | undefined,
) {
  const textTracks = getTextTracks(player)

  if (!textTracks) {
    return
  }

  if (!preference || preference.mode === 'disabled') {
    for (let index = 0; index < textTracks.length; index += 1) {
      const track = textTracks[index]

      if (track && isDisplayTextTrack(track)) {
        track.mode = 'disabled'
      }
    }

    return
  }

  const matchingTrack = findBestMatchingTrack(textTracks, preference, isDisplayTextTrack)

  if (!matchingTrack) {
    return
  }

  for (let index = 0; index < textTracks.length; index += 1) {
    const track = textTracks[index]

    if (track && isDisplayTextTrack(track)) {
      track.mode = track === matchingTrack ? 'showing' : 'disabled'
    }
  }
}

/**
 * Restores the text track settings based on preference.
 *
 * @param player - Video.js player instance.
 * @param settings - Text track settings to restore.
 */
function restoreTextTrackSettings(
  player: VideoJsPlayer,
  settings: VideoJsTextTrackSettings | null,
) {
  if (!settings) {
    return
  }

  const textTrackSettings = getTextTrackSettings(player)

  if (!textTrackSettings) {
    return
  }

  textTrackSettings.setValues?.(settings)
  textTrackSettings.updateDisplay?.()
}

/**
 * Captures the part of the player UI state that should survive a source switch.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param preferredQualityLabel Explicit manual quality preference when already known.
 * @returns Snapshot of the reusable player state.
 */
export function capturePlayerState(
  player: VideoJsPlayer,
  preferredQualityLabel: string | null,
): VideoJsPlayerState {
  const playerElement = player.el()
  const volume = player.volume()
  const muted = player.muted()
  const playbackRate = player.playbackRate()
  const isFullscreen = player.isFullscreen()
  const qualityLabel = normalizeQualityPreferenceLabel(
    preferredQualityLabel ?? getSelectedQualityLabel(player),
  )

  return {
    volume: typeof volume === 'number' && Number.isFinite(volume) ? volume : 1,
    muted: Boolean(muted),
    playbackRate:
      typeof playbackRate === 'number' && Number.isFinite(playbackRate) && playbackRate > 0
        ? playbackRate
        : 1,
    qualityLabel,
    audioTrack: captureSelectedAudioTrack(player),
    textTrack: captureSelectedTextTrack(player),
    textTrackSettings: captureTextTrackSettings(player),
    isFullscreen: Boolean(isFullscreen),
    showsRemainingTime: playerElement instanceof HTMLElement
      ? playerElement.classList.contains(REMAINING_TIME_CLASS)
      : false,
  }
}

/**
 * Resolves the quality label that should survive one source switch.
 *
 * @param preferredQualityLabel Manual quality preference captured during the session.
 * @param playerState Persisted Video.js state snapshot captured before the switch.
 * @returns Preferred quality label for the next source.
 */
export function resolveRetainedQualityLabel(
  preferredQualityLabel: string | null,
  playerState: VideoJsPlayerState | null,
): string | null {
  const normalizedPreferredQualityLabel = normalizeQualityPreferenceLabel(preferredQualityLabel)

  if (normalizedPreferredQualityLabel) {
    return normalizedPreferredQualityLabel
  }

  return normalizeQualityPreferenceLabel(playerState?.qualityLabel ?? null)
}

/**
 * Synchronizes the accessible labels applied to the clickable timer control.
 *
 * @param playerElement Root player element enhanced by Video.js.
 * @param currentTimeElement Native elapsed time control.
 * @param remainingTimeElement Native remaining time control.
 */
export function syncTimerToggleState(
  playerElement: HTMLElement,
  currentTimeElement: HTMLElement,
  remainingTimeElement: HTMLElement,
) {
  const nextActionLabel = playerElement.classList.contains(REMAINING_TIME_CLASS)
    ? t('player.showElapsedTime') : t('player.showRemainingTime')

  for (const timerElement of [currentTimeElement, remainingTimeElement]) {
    timerElement.setAttribute('role', 'button')
    timerElement.tabIndex = 0
    timerElement.setAttribute('title', nextActionLabel)
    timerElement.setAttribute('aria-label', nextActionLabel)
  }
}

/**
 * Synchronizes the remaining-time display mode from one stored player state.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param showsRemainingTime Indicates whether the remaining timer should stay visible.
 */
export function restoreTimerDisplayState(player: VideoJsPlayer, showsRemainingTime: boolean) {
  const playerElement = player.el()

  if (!(playerElement instanceof HTMLElement)) {
    return
  }

  playerElement.classList.toggle(REMAINING_TIME_CLASS, showsRemainingTime)

  const currentTimeElement = playerElement.querySelector<HTMLElement>('.vjs-current-time')
  const remainingTimeElement = playerElement.querySelector<HTMLElement>('.vjs-remaining-time')

  if (currentTimeElement && remainingTimeElement) {
    syncTimerToggleState(playerElement, currentTimeElement, remainingTimeElement)
  }
}

/**
 * Applies one persisted player state after a source switch or remount.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param state Persisted player state that should be restored.
 */
export function applyPersistedPlayerState(
  player: VideoJsPlayer,
  state: VideoJsPlayerState | null,
) {
  if (!state) {
    return
  }

  if (Number.isFinite(state.volume)) {
    player.volume(Math.min(Math.max(state.volume, 0), 1))
  }

  player.muted(state.muted)

  if (Number.isFinite(state.playbackRate) && state.playbackRate > 0) {
    player.playbackRate(state.playbackRate)
  }

  restoreAudioTrack(player, state.audioTrack)
  restoreTextTrackSettings(player, state.textTrackSettings)
  restoreTextTrack(player, state.textTrack)
  restoreTimerDisplayState(player, state.showsRemainingTime)
}

/**
 * Returns the source URL currently associated with a Video.js playback event.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param fallbackSourceUrl Source URL used when Video.js does not report one.
 * @returns Source URL reported by Video.js, or the provided fallback.
 */
export function getPlaybackEventSourceUrl(
  player: VideoJsPlayer,
  fallbackSourceUrl: string,
): string | null {
  const currentSource = player.currentSource() as VideoJsCurrentSource | null
  const sourceUrl = currentSource?.src ?? fallbackSourceUrl

  return sourceUrl || null
}
