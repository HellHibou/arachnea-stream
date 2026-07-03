import type {
  QualityLevelListHandle,
  QualityMenuButtonComponent,
  VideoJsMenuButtonComponent,
  VideoJsPlayer,
  VhsHandlerHandle,
  VhsPlaylist,
} from '@/composables/video/video-js-media-renderer/types'

/**
 * Gets a menu button component from the player control bar by trying multiple possible names.
 *
 * @param player - Video.js player instance.
 * @param childNames - Array of possible child names for the menu button.
 * @returns Menu button component or null if not found.
 */
function getControlBarMenuButton(
  player: VideoJsPlayer,
  childNames: string[],
): VideoJsMenuButtonComponent | null {
  const controlBar = player.getChild('controlBar')

  for (const childName of childNames) {
    const menuButton = controlBar?.getChild(childName) as VideoJsMenuButtonComponent | null

    if (menuButton) {
      return menuButton
    }
  }

  return null
}

/** Options for syncing displayed quality preference. */
interface SyncDisplayedQualityPreferenceOptions {
  /**
   * When true, a missing quality menu means the manual preference cannot apply.
   */
  treatMissingMenuAsUnavailable?: boolean
}

/**
 * Returns the quality menu button component when the plugin mounted it successfully.
 *
 * @param player Video.js player currently bound to the renderer.
 * @returns Quality menu button instance, or `null` when unavailable.
 */
export function getQualityMenuButton(player: VideoJsPlayer): QualityMenuButtonComponent | null {
  return getControlBarMenuButton(player, ['QualityMenuButton']) as
    | QualityMenuButtonComponent
    | null
}

/**
 * Returns the quality label currently selected in the Video.js quality menu.
 *
 * @param player Video.js player currently bound to the renderer.
 * @returns Selected quality label, or `null` when unavailable.
 */
export function getSelectedQualityLabel(player: VideoJsPlayer): string | null {
  const qualityMenuButton = getQualityMenuButton(player)
  const selectedItem = qualityMenuButton?.items?.find((item) =>
    item.selected_ || item.hasClass?.('vjs-selected'),
  )
  const qualityLabel = selectedItem?.options_?.label?.trim()

  return qualityLabel || null
}

/**
 * Normalizes a menu label to the persisted quality preference value.
 *
 * @param qualityLabel Quality label displayed by the Video.js quality menu.
 * @returns Manual quality label, or `null` when automatic quality should be used.
 */
export function normalizeQualityPreferenceLabel(qualityLabel: string | null): string | null {
  if (!qualityLabel) {
    return null
  }

  const normalizedQualityLabel = qualityLabel.trim()
  return normalizedQualityLabel && normalizedQualityLabel !== 'Auto'
    ? normalizedQualityLabel
    : null
}

/**
 * Normalizes one displayed quality label to the token expected by the plugin default option.
 *
 * @param qualityLabel Previously selected quality label.
 * @returns Default-resolution token understood by the quality-menu plugin.
 */
export function resolveQualityPreferenceToken(qualityLabel: string | null): string | null {
  const normalizedQualityLabel = normalizeQualityPreferenceLabel(qualityLabel)

  if (!normalizedQualityLabel) {
    return null
  }

  const resolutionMatch = normalizedQualityLabel.match(/(\d+)\s*p/i)

  if (resolutionMatch?.[1]) {
    return resolutionMatch[1]
  }

  if (normalizedQualityLabel.includes('HD')) {
    return 'HD'
  }

  if (normalizedQualityLabel.includes('SD')) {
    return 'SD'
  }

  return normalizedQualityLabel
}

/**
 * Gets the list of VHS playlists that can be selected.
 *
 * @param vhs - VHS handler from the player.
 * @returns Array of selectable playlists.
 */
function getSelectableVhsPlaylists(vhs: VhsHandlerHandle): VhsPlaylist[] {
  const playlists = vhs.playlists?.main?.playlists ?? []
  const compatiblePlaylists = playlists.filter((playlist) => playlist.excludeUntil !== Infinity)
  const enabledPlaylists = compatiblePlaylists.filter((playlist) =>
    !playlist.disabled &&
    !(typeof playlist.excludeUntil === 'number' && playlist.excludeUntil > Date.now()),
  )

  if (enabledPlaylists.length > 0) {
    return enabledPlaylists
  }

  return compatiblePlaylists.filter((playlist) => !playlist.disabled)
}

/**
 * Gets the height of a VHS playlist from its resolution attributes.
 *
 * @param playlist - VHS playlist to get height from.
 * @returns Playlist height in pixels or null.
 */
function getVhsPlaylistHeight(playlist: VhsPlaylist): number | null {
  const height = playlist.attributes?.RESOLUTION?.height
  return typeof height === 'number' && Number.isFinite(height) ? height : null
}

/**
 * Gets the bandwidth of a VHS playlist from its attributes.
 *
 * @param playlist - VHS playlist to get bandwidth from.
 * @returns Playlist bandwidth in bits per second, or maximum safe integer as fallback.
 */
function getVhsPlaylistBandwidth(playlist: VhsPlaylist): number {
  const bandwidth = playlist.attributes?.BANDWIDTH

  if (typeof bandwidth === 'number' && Number.isFinite(bandwidth) && bandwidth > 0) {
    return bandwidth
  }

  return Number.MAX_SAFE_INTEGER
}

/**
 * Picks the preferred playlist based on available bandwidth.
 *
 * @param playlists - Array of playlists to choose from.
 * @param systemBandwidth - Current estimated system bandwidth in bits per second.
 * @returns Preferred playlist or null if none available.
 */
function pickPreferredPlaylistByBandwidth(
  playlists: VhsPlaylist[],
  systemBandwidth: number | undefined,
): VhsPlaylist | null {
  if (playlists.length === 0) {
    return null
  }

  const sortedPlaylists = [...playlists].sort(
    (left, right) => getVhsPlaylistBandwidth(left) - getVhsPlaylistBandwidth(right),
  )

  if (
    typeof systemBandwidth !== 'number' ||
    !Number.isFinite(systemBandwidth) ||
    systemBandwidth <= 0
  ) {
    return sortedPlaylists[sortedPlaylists.length - 1] ?? null
  }

  const matchingBandwidthPlaylists = sortedPlaylists.filter(
    (playlist) => getVhsPlaylistBandwidth(playlist) <= systemBandwidth,
  )

  if (matchingBandwidthPlaylists.length > 0) {
    return matchingBandwidthPlaylists[matchingBandwidthPlaylists.length - 1] ?? null
  }

  return sortedPlaylists[0] ?? null
}

/**
 * Chooses one VHS playlist matching the preferred manual quality label when possible.
 *
 * @param vhs VHS handler currently attached to the player.
 * @param qualityPreferenceToken Normalized token derived from the selected menu label.
 * @returns One matching playlist, or `null` when the preference cannot be satisfied.
 */
export function selectPreferredVhsPlaylist(
  vhs: VhsHandlerHandle,
  qualityPreferenceToken: string,
): VhsPlaylist | null {
  const selectablePlaylists = getSelectableVhsPlaylists(vhs)

  if (selectablePlaylists.length === 0) {
    return null
  }

  if (/^\d+$/.test(qualityPreferenceToken)) {
    const preferredHeight = Number.parseInt(qualityPreferenceToken, 10)
    const matchingHeightPlaylists = selectablePlaylists.filter(
      (playlist) => getVhsPlaylistHeight(playlist) === preferredHeight,
    )

    return pickPreferredPlaylistByBandwidth(matchingHeightPlaylists, vhs.systemBandwidth)
  }

  if (qualityPreferenceToken === 'HD') {
    const hdPlaylists = selectablePlaylists.filter((playlist) => {
      const height = getVhsPlaylistHeight(playlist)

      if (typeof height === 'number') {
        return height >= 720
      }

      return getVhsPlaylistBandwidth(playlist) >= 2_000_000
    })

    return pickPreferredPlaylistByBandwidth(hdPlaylists, vhs.systemBandwidth)
  }

  if (qualityPreferenceToken === 'SD') {
    const sdPlaylists = selectablePlaylists.filter((playlist) => {
      const height = getVhsPlaylistHeight(playlist)

      if (typeof height === 'number') {
        return height < 720
      }

      return getVhsPlaylistBandwidth(playlist) < 2_000_000
    })

    return pickPreferredPlaylistByBandwidth(sdPlaylists, vhs.systemBandwidth)
  }

  return null
}

/**
 * Mirrors the manual quality preference in the menu UI without triggering another rendition switch.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param qualityLabel Preferred quality label to display as selected.
 * @param options Controls how pending quality-menu initialization is interpreted.
 * @returns `true` when the preference is Auto or exists in the current menu.
 */
export function syncDisplayedQualityPreference(
  player: VideoJsPlayer,
  qualityLabel: string | null,
  options: SyncDisplayedQualityPreferenceOptions = {},
): boolean {
  const normalizedQualityLabel = normalizeQualityPreferenceLabel(qualityLabel)

  if (!normalizedQualityLabel) {
    return true
  }

  const qualityMenuButton = getQualityMenuButton(player)
  const qualityLevels = typeof player.qualityLevels === 'function'
    ? player.qualityLevels() as QualityLevelListHandle
    : null
  const menuItems = qualityMenuButton?.items ?? []

  if (!qualityLevels || menuItems.length === 0) {
    return !options.treatMissingMenuAsUnavailable
  }

  const matchingItem = menuItems.find((item) =>
    normalizeQualityPreferenceLabel(item.options_?.label ?? null) === normalizedQualityLabel,
  )

  if (!matchingItem) {
    return false
  }

  menuItems.forEach((item) => {
    item.selected_ = item === matchingItem

    const isActive = Array.isArray(item.levels_)
      ? item.levels_.includes(qualityLevels.selectedIndex)
      : false

    item.selected?.(isActive)
  })

  return true
}

/**
 * Disables hover behavior on a menu button component.
 *
 * @param menuButtonComponent - Menu button component to disable hover on.
 */
function disableMenuHoverBehavior(menuButtonComponent: VideoJsMenuButtonComponent | null) {
  const menuButton = menuButtonComponent?.menuButton_

  if (!menuButton) {
    return
  }

  menuButton.off('mouseenter')
  menuButton.off('mouseover')
}

/**
 * Keeps the quality menu click-only by removing its hover handlers.
 *
 * @param player Video.js player currently bound to the renderer.
 */
export function disableQualityMenuHoverBehavior(player: VideoJsPlayer) {
  disableMenuHoverBehavior(getQualityMenuButton(player))
}

/**
 * Keeps the audio language menu click-only by removing its hover handlers.
 *
 * @param player Video.js player currently bound to the renderer.
 */
export function disableAudioMenuHoverBehavior(player: VideoJsPlayer) {
  disableMenuHoverBehavior(getControlBarMenuButton(player, [
    'AudioTrackButton',
    'audioTrackButton',
  ]))
}

/**
 * Keeps the subtitle menu click-only by removing its hover handlers.
 *
 * @param player Video.js player currently bound to the renderer.
 */
export function disableSubtitleMenuHoverBehavior(player: VideoJsPlayer) {
  disableMenuHoverBehavior(getControlBarMenuButton(player, [
    'SubsCapsButton',
    'subsCapsButton',
    'SubtitlesButton',
    'subtitlesButton',
    'CaptionsButton',
    'captionsButton',
  ]))
}

/**
 * Keeps Video.js popup menus click-only when their default hover behavior is too eager.
 *
 * @param player Video.js player currently bound to the renderer.
 */
export function disableControlBarMenuHoverBehavior(player: VideoJsPlayer) {
  disableQualityMenuHoverBehavior(player)
  disableAudioMenuHoverBehavior(player)
  disableSubtitleMenuHoverBehavior(player)
}
