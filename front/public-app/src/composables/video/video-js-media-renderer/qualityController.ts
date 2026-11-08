import type { ResolvedVideoMediaSource } from '@/services/players'
import type { VideoJsPlayerState } from '@/types/media'
import type { VideoJsPlayer, VideoJsTechHandle } from './types'
import { installDashQualityBridge, isDashSource, type DashQualityBridge } from './dash'
import {
  getSelectedQualityLabel,
  resolveQualityPreferenceToken,
  selectPreferredVhsPlaylist,
  syncDisplayedQualityPreference,
} from './quality'
import {
  capturePlayerState,
  resolveRetainedQuality,
  restorePersistedTrackPreferences,
} from './state'
import type { VideoJsTrackSelectionConfig } from './trackPreferences'

/** Delay between attempts to attach the asynchronously created dash.js engine. */
const DASH_QUALITY_BRIDGE_RETRY_INTERVAL = 250
/** Maximum time spent waiting for a DASH quality bridge. */
const DASH_QUALITY_BRIDGE_RETRY_TIMEOUT = 10_000

interface QualityControllerOptions {
  getPlayer: () => VideoJsPlayer | null
  getSourceUrl: () => string
  onStateChange: (player: VideoJsPlayer) => void
  getTrackSelectionConfig?: () => VideoJsTrackSelectionConfig | null | undefined
}

/**
 * Owns quality preferences, VHS overrides, DASH bridge retries and their cleanup.
 * @param options Current player/source readers and the persistence callback.
 * @returns The quality operations used by the source controller and player events.
 */
export function createQualityController(options: QualityControllerOptions) {
  const { getPlayer, getSourceUrl, onStateChange, getTrackSelectionConfig } = options
  let preferredQuality: string | null = null
  let retainedPlayerState: VideoJsPlayerState | null = null
  let pendingQualitySelectorCleanup: (() => void) | null = null
  let pendingDashQualityBridgeCleanup: (() => void) | null = null
  let activeDashQualityBridge: DashQualityBridge | null = null

  /** Releases the current VHS selector override or its pending retry. */
  function clearPendingQualitySelectorCleanup() {
    pendingQualitySelectorCleanup?.()
    pendingQualitySelectorCleanup = null
  }

  /** Releases both pending DASH installation and an installed bridge. */
  function clearPendingDashQualityBridge() {
    pendingDashQualityBridgeCleanup?.()
    pendingDashQualityBridgeCleanup = null
    activeDashQualityBridge = null
  }

  /** Releases source-specific quality resources while preserving the preference. */
  function reset() {
    clearPendingQualitySelectorCleanup()
    clearPendingDashQualityBridge()
    retainedPlayerState = null
  }

  /**
   * Retains the next source's UI state and quality preference.
   * @param state State captured before the source transition.
   */
  function retain(state: VideoJsPlayerState | null) {
    preferredQuality = resolveRetainedQuality(preferredQuality, state)
    retainedPlayerState = state
  }

  /**
   * Captures the player with the controller's current quality preference.
   * @param player Player whose UI state should be persisted.
   */
  function captureState(player: VideoJsPlayer) {
    return capturePlayerState(player, preferredQuality)
  }

  /**
   * Observes quality-menu selections and releases pending callbacks on disposal.
   * @param player Ready Video.js player with installed quality controls.
   */
  function attach(player: VideoJsPlayer) {
    const element = player.el()
    const timers = new Set<number>()
    /** Queues preference capture after the clicked menu item updates. */
    const handleClick = (event: Event) => {
      const target = event.target as HTMLElement | null
      if (!target?.closest('.vjs-quality-menu-wrapper .vjs-menu-item')) return
      const timer = window.setTimeout(() => {
        timers.delete(timer)
        if (getPlayer() !== player) return
        preferredQuality = getSelectedQualityLabel(player)
        onStateChange(player)
      }, 0)
      timers.add(timer)
    }
    element?.addEventListener('click', handleClick)
    player.on('dispose', () => {
      element?.removeEventListener('click', handleClick)
      for (const timer of timers) window.clearTimeout(timer)
    })
  }

  /**
   * Reconciles the retained preference with the current source's quality menu.
   * @param player Player receiving the preference.
   * @param source Current media source.
   * @param treatMissingMenuAsUnavailable Whether a missing VHS menu clears the preference.
   */
  function sync(player: VideoJsPlayer, source: ResolvedVideoMediaSource, treatMissingMenuAsUnavailable = false) {
    const isDash = isDashSource(source)
    const available = syncDisplayedQualityPreference(player, preferredQuality, {
      treatMissingMenuAsUnavailable: isDash ? false : treatMissingMenuAsUnavailable,
    })
    if (!available) {
      clearPendingQualitySelectorCleanup()
      preferredQuality = null
      onStateChange(player)
      return
    }
    if (isDash) {
      activeDashQualityBridge?.applyQualityPreference(preferredQuality)
    }
  }

  /**
   * Installs the quality adapter for a newly assigned source.
   * @param player Player receiving the source.
   * @param source Source used to choose DASH or VHS integration.
   */
  function install(player: VideoJsPlayer, source: ResolvedVideoMediaSource) {
    if (isDashSource(source)) {
      installDashQualityBridgeForSource(player, preferredQuality)
    } else {
      configureQualityPreferenceSelector(player, preferredQuality, source.src)
    }
  }

  /**
   * Pins the VHS playlist selector until the current source is released.
   * @param player Active player.
   * @param quality Retained quality preference.
   * @param expectedSourceUrl URL used to reject obsolete selector retries.
   */
  function configureQualityPreferenceSelector(
    player: VideoJsPlayer,
    quality: string | null,
    expectedSourceUrl: string,
  ) {
    clearPendingQualitySelectorCleanup()

    const qualityPreferenceToken = resolveQualityPreferenceToken(quality)

    if (!qualityPreferenceToken) {
      return
    }
    const applySelectorOverride = () => {
      if (getPlayer() !== player || getSourceUrl() !== expectedSourceUrl) {
        return false
      }
      const tech = player.tech(true) as VideoJsTechHandle | null
      const vhs = tech?.vhs

      if (!vhs) {
        return false
      }
      const fallbackSelectPlaylist = vhs.selectPlaylist.bind(vhs)
      vhs.selectPlaylist = function selectPreferredPlaylistWrapper() {
        return selectPreferredVhsPlaylist(
          this as typeof vhs,
          qualityPreferenceToken,
        ) ?? fallbackSelectPlaylist()
      }
      pendingQualitySelectorCleanup = () => {
        const currentTech = player.tech(true) as VideoJsTechHandle | null

        if (currentTech?.vhs === vhs) {
          vhs.selectPlaylist = fallbackSelectPlaylist
        }
        pendingQualitySelectorCleanup = null
      }
      return true
    }
    if (applySelectorOverride()) {
      return
    }
    const retryTimer = window.setTimeout(() => {
      applySelectorOverride()
    }, 0)

    pendingQualitySelectorCleanup = () => {
      window.clearTimeout(retryTimer)
      pendingQualitySelectorCleanup = null
    }
  }
  /**
   * Attaches the dash.js quality bridge to the DASH source just assigned to the player.
   *
   * The source handler creates its dash.js MediaPlayer asynchronously after
   * `player.src()` is applied, so the installation is retried on a short
   * interval until the engine is ready or the bounded window expires. Each
   * source application clears pending attempts, so stale retries never attach
   * a bridge to a replaced source.
   *
   * @param player Video.js player currently bound to the renderer.
   * @param quality Preferred quality captured from the previous video state.
   */
  function installDashQualityBridgeForSource(player: VideoJsPlayer, quality: string | null) {
    clearPendingDashQualityBridge()

    let elapsed = 0

    const retryTimer = window.setInterval(() => {
      elapsed += DASH_QUALITY_BRIDGE_RETRY_INTERVAL

      if (elapsed > DASH_QUALITY_BRIDGE_RETRY_TIMEOUT) {
        clearPendingDashQualityBridge()
        return
      }
      if (getPlayer() !== player) {
        clearPendingDashQualityBridge()
        return
      }
      const bridge = installDashQualityBridge({
        player,
        preferredQuality: quality,
        restoreTrackPreferences: () => {
          if (getPlayer() === player) {
            const trackSelectionConfig = getTrackSelectionConfig?.() ?? undefined
            restorePersistedTrackPreferences(player, retainedPlayerState, trackSelectionConfig)
          }
        },
      })

      if (!bridge) {
        return
      }
      window.clearInterval(retryTimer)
      activeDashQualityBridge = bridge
      pendingDashQualityBridgeCleanup = () => {
        bridge.dispose()
        activeDashQualityBridge = null
        pendingDashQualityBridgeCleanup = null
      }
    }, DASH_QUALITY_BRIDGE_RETRY_INTERVAL)

    pendingDashQualityBridgeCleanup = () => {
      window.clearInterval(retryTimer)
      activeDashQualityBridge = null
      pendingDashQualityBridgeCleanup = null
    }
  }

  return { retain, captureState, attach, sync, install, reset }
}

export type QualityController = ReturnType<typeof createQualityController>
