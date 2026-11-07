import type {
  VideoJsTextTrackPreference,
  VideoJsTrackPreference,
  VideoJsTrackProfile,
  VideoJsTrackProfileAudio,
  VideoJsTrackProfileSubtitles,
} from '@/types/media'
import { getBaseLanguageCode, normalizeLanguageCode } from '@/utils'

import { isDashGeneratedAudioTrackId } from './dash'
import type {
  VideoJsAudioTrackHandle,
  VideoJsAudioTrackListHandle,
  VideoJsDashDescriptor,
  VideoJsDashMediaInfo,
  VideoJsPlayer,
  VideoJsTextTrackHandle,
  VideoJsTextTrackListHandle,
} from './types'

/** Text track kinds that can be displayed as subtitles. */
const DISPLAY_TEXT_TRACK_KINDS = new Set(['captions', 'subtitles'])
/** Audio track kinds that identify an audio-description track. */
const AUDIO_DESCRIPTION_KINDS = new Set(['description', 'main-desc'])
/** Descriptor tokens identifying an audio-description track. */
const AUDIO_DESCRIPTION_VALUE_PATTERN =
  /audio[\s_-]*description|audiodescription|describ(?:es|ing)?[\s_-]*video|descriptive|malvoyant|visually?[\s_-]*impaired/i
/** Label heuristic used only when the engine exposes no reliable AD metadata. */
const AUDIO_DESCRIPTION_LABEL_PATTERN =
  /audio\s*description|audiodescription|descriptive|description|malvoyant|(?:^|[^a-z])ad(?:[^a-z]|$)/i
/** Profile used when no configured profile matches the current source. */
const IMPLICIT_TRACK_PROFILE: VideoJsTrackProfile = {
  audio: 'original',
  subtitles: 'disabled',
}
/** Text track preference representing disabled subtitles. */
const DISABLED_TEXT_TRACK_PREFERENCE: VideoJsTextTrackPreference = {
  id: null,
  language: null,
  label: null,
  kind: null,
  mode: 'disabled',
}

/** Selection configuration read from application parameters. */
export interface VideoJsTrackSelectionConfig {
  /** Ordered audio and subtitle combinations. */
  profiles: readonly VideoJsTrackProfile[]
  /** Subtitle languages applied by `auto` profiles, highest priority first. */
  subtitleLanguagePriority: readonly string[]
  /** Whether audio-description tracks may be used as a last resort. */
  allowAudioDescription: boolean
}

/** Persisted preferences produced by one track selection. */
export interface VideoJsTrackSelectionResult {
  /** Applied audio track preference, or `null` when audio was left untouched. */
  audioTrack: VideoJsTrackPreference | null
  /** Applied subtitle track preference. */
  textTrack: VideoJsTextTrackPreference
}

/** Profile retained for the current source and its resolved audio track. */
export interface VideoJsResolvedTrackProfile {
  /** Profile whose audio target matched the source. */
  profile: VideoJsTrackProfile
  /** Audio track selected by the profile, or `null` when audio is ignored. */
  audioTrack: VideoJsAudioTrackHandle | null
}

/** Internal audio candidate enriched with dash.js metadata. */
interface AudioTrackCandidate {
  /** Video.js track handle. */
  track: VideoJsAudioTrackHandle
  /** Matching dash.js metadata when the source is DASH. */
  dashTrack: VideoJsDashMediaInfo | null
  /** Original manifest/list position. */
  order: number
  /** Whether the track was detected as an audio-description track. */
  isAudioDescription: boolean
  /** Role-based preference rank (main first, AD last). */
  rank: number
}

/**
 * Returns a trimmed track field value.
 *
 * @param value Raw track field.
 * @returns Trimmed string, or `null` when empty.
 */
function normalizeTrackField(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null
}

/**
 * Returns the video player audio track list.
 *
 * @param player Video.js player instance.
 * @returns Audio track list handle, or `null` when unavailable.
 */
function getAudioTracks(player: VideoJsPlayer): VideoJsAudioTrackListHandle | null {
  return (player.audioTracks?.() as unknown as VideoJsAudioTrackListHandle | undefined) ?? null
}

/**
 * Returns the video player text track list.
 *
 * @param player Video.js player instance.
 * @returns Text track list handle, or `null` when unavailable.
 */
function getTextTracks(player: VideoJsPlayer): VideoJsTextTrackListHandle | null {
  return (player.textTracks?.() as unknown as VideoJsTextTrackListHandle | undefined) ?? null
}

/**
 * Copies an array-like track list into a plain array.
 *
 * @param tracks Array-like track collection.
 * @returns Existing track handles in list order.
 */
function readTracks<T>(tracks: ArrayLike<T> | null): T[] {
  const result: T[] = []
  if (!tracks) {
    return result
  }

  for (let index = 0; index < tracks.length; index += 1) {
    const track = tracks[index]
    if (track) {
      result.push(track)
    }
  }

  return result
}

/**
 * Checks whether a text track can display subtitles.
 *
 * @param track Text track to check.
 * @returns True for captions and subtitles.
 */
function isDisplayTextTrack(track: VideoJsTextTrackHandle): boolean {
  return Boolean(track.kind && DISPLAY_TEXT_TRACK_KINDS.has(track.kind))
}

/**
 * Reads the audio track metadata exposed by dash.js.
 *
 * @param player Video.js player instance.
 * @returns Audio tracks known by the active DASH engine, or an empty list.
 */
function readDashAudioTracks(player: VideoJsPlayer): VideoJsDashMediaInfo[] {
  try {
    const tracks = player.dash?.mediaPlayer?.getTracksFor?.('audio')
    return Array.isArray(tracks) ? tracks : []
  } catch {
    return []
  }
}

/**
 * Finds the dash.js metadata matching one Video.js audio track.
 *
 * @param track Video.js audio track.
 * @param dashTracks Audio tracks exposed by dash.js.
 * @returns Matching dash.js track, or `null` when the track is not a DASH track.
 */
function findDashAudioInfo(
  track: VideoJsAudioTrackHandle,
  dashTracks: readonly VideoJsDashMediaInfo[],
): VideoJsDashMediaInfo | null {
  if (!track.id) {
    return null
  }

  return dashTracks.find(
    (candidate) => typeof candidate.index === 'number'
      && `dash-audio-${candidate.index}` === track.id,
  ) ?? null
}

/**
 * Collects normalized values and scheme URIs from dash.js descriptors.
 *
 * @param values Plain value list such as roles or accessibility.
 * @param descriptors Scheme-based descriptors.
 * @returns Lowercase descriptor tokens.
 */
function readDashDescriptorTokens(
  values: readonly string[] | null | undefined,
  descriptors: readonly VideoJsDashDescriptor[] | null | undefined,
): string[] {
  return [
    ...(values ?? []),
    ...(descriptors ?? []).flatMap((descriptor) => [
      descriptor.value ?? '',
      descriptor.schemeIdUri ?? '',
    ]),
  ]
    .map((value) => value.trim().toLowerCase())
    .filter(Boolean)
}

/**
 * Checks whether one dash.js audio track declares a given role.
 *
 * @param dashTrack dash.js audio metadata.
 * @param role Role to search, such as `main` or `alternate`.
 * @returns True when the role or its scheme descriptor contains the role.
 */
function hasDashAudioRole(dashTrack: VideoJsDashMediaInfo | null, role: string): boolean {
  if (!dashTrack) {
    return false
  }

  const roles = [
    ...(dashTrack.roles ?? []),
    ...(dashTrack.rolesWithSchemeIdUri ?? []).map((descriptor) => descriptor.value ?? ''),
  ]

  return roles.some((value) => value.trim().toLowerCase() === role)
}

/**
 * Detects whether one audio track is an audio-description track.
 *
 * The detection combines the Video.js kind exposed by HLS/WebVTT handlers,
 * dash.js roles and accessibility descriptors, and a label heuristic kept as a
 * last resort.
 *
 * @param track Video.js audio track.
 * @param dashTrack Matching dash.js metadata when available.
 * @returns True when the track is detected as audio description.
 */
export function isAudioDescriptionTrack(
  track: VideoJsAudioTrackHandle,
  dashTrack: VideoJsDashMediaInfo | null = null,
): boolean {
  const kind = normalizeTrackField(track.kind)?.toLowerCase()
  if (kind && AUDIO_DESCRIPTION_KINDS.has(kind)) {
    return true
  }

  if (dashTrack) {
    const descriptorTokens = [
      ...readDashDescriptorTokens(dashTrack.roles, dashTrack.rolesWithSchemeIdUri),
      ...readDashDescriptorTokens(dashTrack.accessibility, dashTrack.accessibilitiesWithSchemeIdUri),
    ]

    if (descriptorTokens.some(
      (token) => token === 'description'
        || token === 'main-desc'
        || AUDIO_DESCRIPTION_VALUE_PATTERN.test(token),
    )) {
      return true
    }

    const labels = (dashTrack.labels ?? [])
      .map((label) => label.text ?? '')
      .filter(Boolean)

    if (labels.some((label) => AUDIO_DESCRIPTION_LABEL_PATTERN.test(label))) {
      return true
    }
  }

  const label = normalizeTrackField(track.label)
  return Boolean(label && AUDIO_DESCRIPTION_LABEL_PATTERN.test(label))
}

/**
 * Reads the normalized language of an audio track.
 *
 * @param track Video.js audio track.
 * @param dashTrack Matching dash.js metadata.
 * @returns Normalized language code, or `null` when absent.
 */
function getAudioTrackLanguage(
  track: VideoJsAudioTrackHandle,
  dashTrack: VideoJsDashMediaInfo | null,
): string | null {
  return normalizeLanguageCode(track.language) ?? normalizeLanguageCode(dashTrack?.lang)
}

/**
 * Computes the role-based rank of an audio candidate.
 *
 * @param track Video.js audio track.
 * @param dashTrack Matching dash.js metadata.
 * @param isAudioDescription Whether the track is an audio-description track.
 * @returns Higher values for preferred tracks.
 */
function getAudioTrackRank(
  track: VideoJsAudioTrackHandle,
  dashTrack: VideoJsDashMediaInfo | null,
  isAudioDescription: boolean,
): number {
  if (isAudioDescription) {
    return 0
  }

  const kind = normalizeTrackField(track.kind)?.toLowerCase()

  if (kind === 'main' || hasDashAudioRole(dashTrack, 'main')) {
    return 4
  }

  if (kind === 'alternative' || hasDashAudioRole(dashTrack, 'alternate')) {
    return 3
  }

  return 2
}

/**
 * Builds the ranked audio candidates for the current source.
 *
 * @param player Video.js player instance.
 * @returns Audio candidates with DASH metadata and AD detection.
 */
function readAudioCandidates(player: VideoJsPlayer): AudioTrackCandidate[] {
  const dashTracks = readDashAudioTracks(player)
  const tracks = readTracks(getAudioTracks(player))

  return tracks.map((track, order) => {
    const dashTrack = findDashAudioInfo(track, dashTracks)
    const isAudioDescription = isAudioDescriptionTrack(track, dashTrack)

    return {
      track,
      dashTrack,
      order,
      isAudioDescription,
      rank: getAudioTrackRank(track, dashTrack, isAudioDescription),
    }
  })
}

/**
 * Orders candidates by role, then by dash.js selection priority, then by manifest order.
 *
 * @param left First candidate.
 * @param right Second candidate.
 * @returns Negative when the left candidate is preferred.
 */
function compareAudioCandidates(left: AudioTrackCandidate, right: AudioTrackCandidate): number {
  const rankDifference = right.rank - left.rank
  if (rankDifference) {
    return rankDifference
  }

  const priorityDifference =
    (right.dashTrack?.selectionPriority ?? 0) - (left.dashTrack?.selectionPriority ?? 0)
  if (priorityDifference) {
    return priorityDifference
  }

  return left.order - right.order
}

/**
 * Selects the audio track targeted by one profile.
 *
 * Audio-description tracks are excluded unless they are explicitly allowed or
 * the source exposes a single audio track, in which case they remain a last
 * resort so playback never becomes silent.
 *
 * @param player Video.js player instance.
 * @param target Language code, `original`, or `null` to ignore audio.
 * @param allowAudioDescription Whether AD tracks may be used when alternatives exist.
 * @returns Matching audio track, or `null` when the target is unavailable.
 */
export function findProfileAudioTrack(
  player: VideoJsPlayer,
  target: VideoJsTrackProfileAudio,
  allowAudioDescription: boolean,
): VideoJsAudioTrackHandle | null {
  if (target === null) {
    return null
  }

  const candidates = readAudioCandidates(player)
  if (!candidates.length) {
    return null
  }

  const nonAdCandidates = candidates.filter((candidate) => !candidate.isAudioDescription)
  const usableCandidates = allowAudioDescription
    ? candidates
    : nonAdCandidates.length
      ? nonAdCandidates
      : candidates.length === 1
        ? candidates
        : []

  if (!usableCandidates.length) {
    return null
  }

  if (target === 'original') {
    const mainCandidate = usableCandidates.find((candidate) => candidate.rank >= 4)
    if (mainCandidate) {
      return mainCandidate.track
    }

    const nonAdCandidate = usableCandidates.find((candidate) => !candidate.isAudioDescription)
    return (nonAdCandidate ?? usableCandidates[0])?.track ?? null
  }

  const targetLanguage = normalizeLanguageCode(target)
  if (!targetLanguage) {
    return null
  }

  const nonAdMatch = findAudioLanguageMatch(nonAdCandidates, targetLanguage)
  if (nonAdMatch) {
    return nonAdMatch
  }

  return findAudioLanguageMatch(
    usableCandidates.filter((candidate) => candidate.isAudioDescription),
    targetLanguage,
  )
}

/**
 * Finds the best audio candidate matching a target language.
 *
 * Exact language matches are preferred over base-language matches, then role
 * rank and manifest order break ties.
 *
 * @param candidates Candidates to search.
 * @param targetLanguage Normalized target language.
 * @returns Best matching track, or `null` when no candidate matches.
 */
function findAudioLanguageMatch(
  candidates: readonly AudioTrackCandidate[],
  targetLanguage: string,
): VideoJsAudioTrackHandle | null {
  const exactMatches = candidates.filter(
    (candidate) => getAudioTrackLanguage(candidate.track, candidate.dashTrack) === targetLanguage,
  )

  if (exactMatches.length) {
    return exactMatches.sort(compareAudioCandidates)[0]?.track ?? null
  }

  const targetBaseLanguage = getBaseLanguageCode(targetLanguage)
  if (!targetBaseLanguage) {
    return null
  }

  const baseMatches = candidates.filter(
    (candidate) => getBaseLanguageCode(
      getAudioTrackLanguage(candidate.track, candidate.dashTrack),
    ) === targetBaseLanguage,
  )

  return baseMatches.sort(compareAudioCandidates)[0]?.track ?? null
}

/**
 * Selects the subtitle track matching the ordered language priority.
 *
 * Exact language matches win over base-language matches, and human tracks win
 * over automatically translated tracks for the same language.
 *
 * @param player Video.js player instance.
 * @param subtitleLanguagePriority Ordered language codes.
 * @returns Matching subtitle track, or `null` when no priority language exists.
 */
export function findPrioritySubtitleTrack(
  player: VideoJsPlayer,
  subtitleLanguagePriority: readonly string[],
): VideoJsTextTrackHandle | null {
  const tracks = readTracks(getTextTracks(player)).filter(isDisplayTextTrack)
  if (!tracks.length) {
    return null
  }

  for (const priority of subtitleLanguagePriority) {
    const targetLanguage = normalizeLanguageCode(priority)
    if (!targetLanguage) {
      continue
    }

    const exactMatch = findSubtitleTrack(tracks, targetLanguage, false)
    if (exactMatch) {
      return exactMatch
    }

    const targetBaseLanguage = getBaseLanguageCode(targetLanguage)
    if (targetBaseLanguage) {
      const baseMatch = findSubtitleTrack(tracks, targetBaseLanguage, true)
      if (baseMatch) {
        return baseMatch
      }
    }
  }

  return null
}

/**
 * Finds one subtitle track for a language, preferring non-translated tracks.
 *
 * @param tracks Displayable text tracks.
 * @param language Language code to match.
 * @param compareBaseLanguage Whether to compare only the base language subtag.
 * @returns Matching track, or `null`.
 */
function findSubtitleTrack(
  tracks: readonly VideoJsTextTrackHandle[],
  language: string,
  compareBaseLanguage: boolean,
): VideoJsTextTrackHandle | null {
  const matches = tracks.filter((track) => {
    const trackLanguage = normalizeLanguageCode(track.language)
    if (!trackLanguage) {
      return false
    }

    return compareBaseLanguage
      ? getBaseLanguageCode(trackLanguage) === language
      : trackLanguage === language
  })

  return matches.find((track) => track.autoTranslate !== true) ?? matches[0] ?? null
}

/**
 * Calculates a match score between one track and a persisted preference.
 *
 * Identifiers generated by the DASH source handler are ignored so language,
 * kind and label keep driving the match.
 *
 * @param track Track to match.
 * @param preference Persisted track preference.
 * @returns Match score from 0 to 100.
 */
function getTrackMatchScore(
  track: VideoJsAudioTrackHandle | VideoJsTextTrackHandle,
  preference: VideoJsTrackPreference,
): number {
  const id = normalizeTrackField(track.id)
  const kind = normalizeTrackField(track.kind)
  const label = normalizeTrackField(track.label)

  if (preference.id && id === preference.id && !isDashGeneratedAudioTrackId(preference.id)) {
    return 100
  }

  let score = 0
  const preferenceLanguage = normalizeLanguageCode(preference.language)
  const trackLanguage = normalizeLanguageCode(track.language)

  if (preferenceLanguage && trackLanguage) {
    if (trackLanguage === preferenceLanguage) {
      score += 30
    } else if (getBaseLanguageCode(trackLanguage) === getBaseLanguageCode(preferenceLanguage)) {
      score += 25
    }
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
 * Finds the best audio track matching one persisted audio preference.
 *
 * @param player Video.js player instance.
 * @param preference Persisted audio preference.
 * @returns Best matching track, or `null` when no identity matches the source.
 */
export function findMatchingAudioTrack(
  player: VideoJsPlayer,
  preference: VideoJsTrackPreference | null | undefined,
): VideoJsAudioTrackHandle | null {
  if (!preference) {
    return null
  }

  let matchingTrack: VideoJsAudioTrackHandle | null = null
  let matchingScore = 0

  for (const track of readTracks(getAudioTracks(player))) {
    const score = getTrackMatchScore(track, preference)
    if (score > matchingScore) {
      matchingTrack = track
      matchingScore = score
    }
  }

  return matchingTrack
}

/**
 * Finds the best subtitle track matching one persisted text preference.
 *
 * @param player Video.js player instance.
 * @param preference Persisted text preference.
 * @returns Best matching displayable track, or `null` when no identity matches.
 */
export function findMatchingTextTrack(
  player: VideoJsPlayer,
  preference: VideoJsTextTrackPreference | null | undefined,
): VideoJsTextTrackHandle | null {
  if (!preference || preference.mode === 'disabled') {
    return null
  }

  let matchingTrack: VideoJsTextTrackHandle | null = null
  let matchingScore = 0

  for (const track of readTracks(getTextTracks(player))) {
    if (!isDisplayTextTrack(track)) {
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
 * Creates a persisted preference from one audio or text track.
 *
 * @param track Track handle.
 * @returns Track preference with normalized fields.
 */
export function createTrackPreference(
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
 * Creates a persisted text preference from one text track.
 *
 * @param track Text track handle.
 * @returns Text track preference using the track's current mode.
 */
export function createTextTrackPreference(
  track: VideoJsTextTrackHandle,
): VideoJsTextTrackPreference {
  return {
    ...createTrackPreference(track),
    mode: track.mode === 'showing' ? 'showing' : 'disabled',
  }
}

/**
 * Checks whether two audio track preferences describe the same track.
 *
 * @param left First preference.
 * @param right Second preference.
 * @returns True when both preferences carry the same identity.
 */
export function areTrackPreferencesEqual(
  left: VideoJsTrackPreference | null | undefined,
  right: VideoJsTrackPreference | null | undefined,
): boolean {
  if (!left || !right) {
    return !left && !right
  }

  return normalizeTrackField(left.id) === normalizeTrackField(right.id)
    && normalizeLanguageCode(left.language) === normalizeLanguageCode(right.language)
    && normalizeTrackField(left.label) === normalizeTrackField(right.label)
    && normalizeTrackField(left.kind) === normalizeTrackField(right.kind)
}

/**
 * Checks whether two text track preferences describe the same track and mode.
 *
 * @param left First preference.
 * @param right Second preference.
 * @returns True when both preferences carry the same identity and mode.
 */
export function areTextTrackPreferencesEqual(
  left: VideoJsTextTrackPreference | null | undefined,
  right: VideoJsTextTrackPreference | null | undefined,
): boolean {
  if (!left || !right) {
    return !left && !right
  }

  return left.mode === right.mode && areTrackPreferencesEqual(left, right)
}

/**
 * Enables exactly one audio track.
 *
 * @param player Video.js player instance.
 * @param track Track to enable, or `null` to leave audio untouched.
 */
export function applyAudioTrackSelection(
  player: VideoJsPlayer,
  track: VideoJsAudioTrackHandle | null,
): void {
  if (!track) {
    return
  }

  for (const candidate of readTracks(getAudioTracks(player))) {
    candidate.enabled = candidate === track
  }
}

/**
 * Disables every displayable subtitle track.
 *
 * @param player Video.js player instance.
 */
export function disableDisplayableTextTracks(player: VideoJsPlayer): void {
  for (const track of readTracks(getTextTracks(player))) {
    if (isDisplayTextTrack(track)) {
      track.mode = 'disabled'
    }
  }
}

/**
 * Shows one subtitle track and disables the other displayable tracks.
 *
 * @param player Video.js player instance.
 * @param track Subtitle track to show, or `null` to disable all subtitles.
 * @returns Applied text track preference.
 */
export function applyTextTrackSelection(
  player: VideoJsPlayer,
  track: VideoJsTextTrackHandle | null,
): VideoJsTextTrackPreference {
  if (!track) {
    disableDisplayableTextTracks(player)
    return { ...DISABLED_TEXT_TRACK_PREFERENCE }
  }

  for (const candidate of readTracks(getTextTracks(player))) {
    if (isDisplayTextTrack(candidate)) {
      candidate.mode = candidate === track ? 'showing' : 'disabled'
    }
  }

  return createTextTrackPreference(track)
}

/**
 * Resolves the first configured profile whose audio target is available.
 *
 * Profiles ordered before the first match are skipped, and an implicit
 * `original` + disabled-subtitles profile is returned when none matches.
 *
 * @param player Video.js player instance.
 * @param profiles Ordered profiles.
 * @param allowAudioDescription Whether AD tracks may be used as a last resort.
 * @returns Profile retained for the current source.
 */
export function resolveTrackProfile(
  player: VideoJsPlayer,
  profiles: readonly VideoJsTrackProfile[],
  allowAudioDescription: boolean,
): VideoJsResolvedTrackProfile {
  for (const profile of profiles) {
    if (!profile) {
      continue
    }

    if (profile.audio === null) {
      return { profile, audioTrack: null }
    }

    const audioTrack = findProfileAudioTrack(player, profile.audio, allowAudioDescription)
    if (audioTrack) {
      return { profile, audioTrack }
    }
  }

  return {
    profile: IMPLICIT_TRACK_PROFILE,
    audioTrack: findProfileAudioTrack(
      player,
      IMPLICIT_TRACK_PROFILE.audio,
      allowAudioDescription,
    ),
  }
}

/**
 * Applies one resolved profile's audio and subtitle behaviour.
 *
 * @param player Video.js player instance.
 * @param resolved Resolved profile and its audio track.
 * @param subtitleLanguagePriority Ordered subtitle languages used by `auto` profiles.
 * @returns Preferences applied by the profile.
 */
export function applyResolvedTrackProfile(
  player: VideoJsPlayer,
  resolved: VideoJsResolvedTrackProfile,
  subtitleLanguagePriority: readonly string[],
): VideoJsTrackSelectionResult {
  applyAudioTrackSelection(player, resolved.audioTrack)

  return {
    audioTrack: resolved.audioTrack ? createTrackPreference(resolved.audioTrack) : null,
    textTrack: applyProfileSubtitles(
      player,
      resolved.profile.subtitles,
      subtitleLanguagePriority,
    ),
  }
}

/**
 * Applies the subtitle behaviour declared by one profile.
 *
 * @param player Video.js player instance.
 * @param mode Profile subtitle behaviour.
 * @param subtitleLanguagePriority Ordered subtitle languages used by `auto`.
 * @returns Applied text track preference.
 */
function applyProfileSubtitles(
  player: VideoJsPlayer,
  mode: VideoJsTrackProfileSubtitles,
  subtitleLanguagePriority: readonly string[],
): VideoJsTextTrackPreference {
  if (mode === 'auto') {
    const track = findPrioritySubtitleTrack(player, subtitleLanguagePriority)
    if (track) {
      return applyTextTrackSelection(player, track)
    }
  }

  return applyTextTrackSelection(player, null)
}

/**
 * Resolves and applies the first applicable ordered track profile.
 *
 * @param player Video.js player instance.
 * @param config Track selection configuration.
 * @returns Preferences applied for the current source.
 */
export function applyTrackProfiles(
  player: VideoJsPlayer,
  config: VideoJsTrackSelectionConfig,
): VideoJsTrackSelectionResult {
  const resolved = resolveTrackProfile(player, config.profiles, config.allowAudioDescription)

  return applyResolvedTrackProfile(
    player,
    resolved,
    config.subtitleLanguagePriority,
  )
}
