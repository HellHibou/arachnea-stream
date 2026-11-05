import { computed, shallowRef, watch } from 'vue'
import { t } from '@/i18n'
import type { EntryPlayer } from '@/types/entry'
import type { EntryPlayerLanguageOption, UseEntryVideoPlayerOptions } from './entryPlayerTypes'

/** Sentinel value used for players without a language code. */
const unknownLanguageKey = '__unknown__'

/**
 * Returns a stable grouping key for one player language.
 *
 * @param player Player rendered by the current built-in player context.
 * @returns Stable language key used by the language selector.
 */
function getPlayerLanguageKey(player: EntryPlayer): string {
  return player.lang?.trim().toLocaleLowerCase() || unknownLanguageKey
}

/**
 * Formats one player language for display in the language selector.
 *
 * @param player Player rendered by the current built-in player context.
 * @returns Human-readable language label.
 */
function formatPlayerLanguageLabel(player: EntryPlayer): string {
  return player.lang?.trim().toLocaleUpperCase() || t('player.undefined')
}

/**
 * Returns a stable preference key for one player across item changes.
 *
 * @param player Player rendered by the current built-in player context.
 * @returns Stable player key used to restore the last selected player when available.
 */
function getPlayerPreferenceKey(player: EntryPlayer): string {
  return player.name?.trim().toLocaleLowerCase() || player.label.trim().toLocaleLowerCase()
}

/**
 * Returns embedded players in backend order.
 *
 * The backend order is used as the default priority when several external
 * iframe players are available for the same playable item or entry.
 *
 * @param players Players rendered by the current built-in player context.
 * @returns New array preserving backend order.
 */
function sortEntryPlayers(players: EntryPlayer[]): EntryPlayer[] {
  return [...players]
}

/**
 * Builds the language selector options from the current player list.
 *
 * @param players Players rendered by the current built-in player context.
 * @returns Deduplicated language options preserving backend order.
 */
function buildEntryPlayerLanguageOptions(
  players: EntryPlayer[],
): EntryPlayerLanguageOption[] {
  const options: EntryPlayerLanguageOption[] = []
  const seen = new Set<string>()

  players.forEach((player) => {
    const key = getPlayerLanguageKey(player)

    if (seen.has(key)) {
      return
    }

    seen.add(key)
    options.push({
      key,
      label: formatPlayerLanguageLabel(player),
    })
  })

  return options
}

interface EntryPlayerSelectionOptions extends Pick<UseEntryVideoPlayerOptions, 'details' | 'selectedPlayableItem'> {
  activateMedia: () => void
}

/**
 * Owns language/player selections and remembered preferences for one entry controller.
 * @param options Reactive entry/item refs and the action selecting media mode.
 * @returns Selector state and explicit reset/observer operations.
 */
export function createEntryPlayerSelection(options: EntryPlayerSelectionOptions) {
  /** The currently selected language key in the language selector. */
  const selectedLanguageKey = shallowRef<string | null>(null)

  /** The currently selected player identifier. */
  const selectedPlayerId = shallowRef<string | null>(null)

  /** The language key selected for the current playable item. */
  const selectedPlayableLanguageKey = shallowRef<string | null>(null)

  /** The player identifier selected for the current playable item. */
  const selectedPlayablePlayerId = shallowRef<string | null>(null)

  /** The preferred language key persisted across playable item changes. */
  const preferredLanguageKey = shallowRef<string | null>(null)

  /** The preferred player key persisted across playable item changes. */
  const preferredPlayerKey = shallowRef<string | null>(null)

  /**
   * Exposes the embedded players returned by the backend entry payload.
   *
   * @returns Array of all players for the entry.
   */
  const availablePlayers = computed<EntryPlayer[]>(() => options.details.value?.players.entries ?? [])

  /**
   * Exposes the players that should drive the built-in player.
   * Uses playable item players if available, otherwise falls back to entry players.
   *
   * @returns Array of players relevant to the current playable context.
   */
  const activePlayers = computed<EntryPlayer[]>(() => {
    const selectedItemPlayers = options.selectedPlayableItem.value?.players.entries ?? []
    return selectedItemPlayers.length > 0 ? selectedItemPlayers : availablePlayers.value
  })

  /**
   * Exposes the language identifier currently selected in the UI.
   * Automatically switches between playable item and entry language selection.
   *
   * @returns The active language key.
   */
  const activeLanguageKey = computed<string | null>({
    get() {
      return options.selectedPlayableItem.value?.players.entries.length
        ? selectedPlayableLanguageKey.value
        : selectedLanguageKey.value
    },
    set(value) {
      if (options.selectedPlayableItem.value?.players.entries.length) {
        selectedPlayableLanguageKey.value = value
        return
      }

      selectedLanguageKey.value = value
    },
  })

  /**
   * Exposes the player identifier currently selected in the UI.
   * Automatically switches between playable item and entry player selection.
   *
   * @returns The active player identifier.
   */
  const activePlayerId = computed<string | null>({
    get() {
      return options.selectedPlayableItem.value?.players.entries.length
        ? selectedPlayablePlayerId.value
        : selectedPlayerId.value
    },
    set(value) {
      if (options.selectedPlayableItem.value?.players.entries.length) {
        selectedPlayablePlayerId.value = value
        return
      }

      selectedPlayerId.value = value
    },
  })

  /**
   * Exposes the distinct language filters available for the current player list.
   *
   * @returns Array of language options for the language selector.
   */
  const availableLanguages = computed(() => buildEntryPlayerLanguageOptions(activePlayers.value))

  /**
   * Exposes the effective language key driving the filtered player list.
   * Falls back to the first available language if the active one is not available.
   *
   * @returns The resolved language key or null.
   */
  const resolvedLanguageKey = computed<string | null>(() => {
    if (availableLanguages.value.length === 0) {
      return null
    }

    if (
      activeLanguageKey.value &&
      availableLanguages.value.some((language) => language.key === activeLanguageKey.value)
    ) {
      return activeLanguageKey.value
    }

    return availableLanguages.value[0]?.key ?? null
  })

  /**
   * Exposes the players available for the selected language.
   *
   * @returns Array of players filtered by the selected language.
   */
  const filteredPlayers = computed<EntryPlayer[]>(() => {
    if (availableLanguages.value.length < 2) {
      return sortEntryPlayers(activePlayers.value)
    }

    const currentLanguageKey = resolvedLanguageKey.value
    return sortEntryPlayers(
      activePlayers.value.filter((player) => getPlayerLanguageKey(player) === currentLanguageKey),
    )
  })

  /**
   * Resolves the embedded player currently selected in the UI.
   *
   * @returns The selected player, or the first filtered player, or null.
   */
  const selectedPlayer = computed<EntryPlayer | null>(() => {
    const players = filteredPlayers.value

    if (!players.length) {
      return null
    }

    return players.find((player) => player.id === activePlayerId.value) ?? players[0] ?? null
  })

  /**
   * Exposes the selected playable item title displayed above the built-in player.
   *
   * @returns The playable item title or null.
   */
  const selectedPlayableTitle = computed(() =>
    options.selectedPlayableItem.value?.title?.trim() || null,
  )

  /**
   * Indicates whether the embedded language selector should be shown.
   *
   * @returns True when multiple languages are available.
   */
  const showLanguageSelector = computed(() => availableLanguages.value.length > 1)

  /**
   * Indicates whether the embedded player selector should be shown.
   *
   * @returns True when multiple players are available for the selected language.
   */
  const showPlayerSelector = computed(() => filteredPlayers.value.length > 1)

  /**
   * Indicates whether at least one selector should be rendered below the player.
   *
   * @returns True when language or player selector should be shown.
   */
  const showPlayerControls = computed(() =>
    showLanguageSelector.value || showPlayerSelector.value,
  )

  /**
   * Clears the selected-item-specific language and player state.
   */
  function clearSelectedPlayablePlayerState(): void {
    selectedPlayableLanguageKey.value = null
    selectedPlayablePlayerId.value = null
  }

  /**
   * Stores the currently selected language as the preferred language for the next playable video.
   */
  function rememberCurrentLanguage(): void {
    preferredLanguageKey.value = resolvedLanguageKey.value
  }

  /**
   * Stores the currently selected player as the preferred player for the next playable video.
   */
  function rememberCurrentPlayer(): void {
    if (!selectedPlayer.value) {
      return
    }

    preferredPlayerKey.value = getPlayerPreferenceKey(selectedPlayer.value)
  }

  /** Clears entry and item selections while retaining cross-item preferences. */
  function resetForEntryLoad() {
    selectedLanguageKey.value = null
    selectedPlayerId.value = null
    clearSelectedPlayablePlayerState()
  }

  /** Applies the remembered language to a freshly loaded entry. */
  function applyLoadedEntryDefaults() {
    selectedLanguageKey.value = preferredLanguageKey.value
    selectedPlayerId.value = null
    clearSelectedPlayablePlayerState()
  }

  /** Installs language and player reconciliation in their original setup order. */
  function observeSelectors() {
    watch(availableLanguages, (languages) => {
      if (languages.length === 0) {
        activeLanguageKey.value = null
        return
      }

      if (languages.some((language) => language.key === activeLanguageKey.value)) {
        return
      }

      if (
        preferredLanguageKey.value &&
        languages.some((language) => language.key === preferredLanguageKey.value)
      ) {
        activeLanguageKey.value = preferredLanguageKey.value
        return
      }

      activeLanguageKey.value = languages[0]?.key ?? null
    }, { immediate: true })
    watch(filteredPlayers, (players) => {
      if (players.length === 0) {
        activePlayerId.value = null
        return
      }

      if (players.some((player) => player.id === activePlayerId.value)) {
        return
      }

      const preferredPlayer = preferredPlayerKey.value
        ? players.find((player) => getPlayerPreferenceKey(player) === preferredPlayerKey.value)
        : null

      activePlayerId.value = preferredPlayer?.id ?? players[0]?.id ?? null
    }, { immediate: true })
  }

  /** Installs item-specific selection restoration after the stream observers. */
  function observePlayableItem() {
    watch(
      () => options.selectedPlayableItem.value?.id ?? null,
      (nextPlayableItemId, previousPlayableItemId) => {
        if (!nextPlayableItemId) {
          clearSelectedPlayablePlayerState()
          return
        }

        if (previousPlayableItemId !== nextPlayableItemId) {
          const selectedItemPlayers = options.selectedPlayableItem.value?.players.entries ?? []
          const nextLanguageOptions = buildEntryPlayerLanguageOptions(selectedItemPlayers)
          const preferredPlayerInAnyLanguage = preferredPlayerKey.value
            ? selectedItemPlayers.find(
              (player) => getPlayerPreferenceKey(player) === preferredPlayerKey.value,
            )
            : null
          const nextLanguageKey =
            preferredLanguageKey.value &&
              nextLanguageOptions.some((language) => language.key === preferredLanguageKey.value)
              ? preferredLanguageKey.value
              : (preferredPlayerInAnyLanguage
                ? getPlayerLanguageKey(preferredPlayerInAnyLanguage)
                : nextLanguageOptions[0]?.key ?? null)
          const playersInSelectedLanguage = selectedItemPlayers.filter(
            (player) => getPlayerLanguageKey(player) === nextLanguageKey,
          )
          const preferredPlayer = preferredPlayerKey.value
            ? playersInSelectedLanguage.find(
              (player) => getPlayerPreferenceKey(player) === preferredPlayerKey.value,
            )
            : null

          selectedPlayableLanguageKey.value = nextLanguageKey
          selectedPlayablePlayerId.value = preferredPlayer?.id ?? playersInSelectedLanguage[0]?.id ?? null
          options.activateMedia()
        }
      },
    )
  }

  return {
    activeLanguageKey, activePlayerId, availableLanguages, filteredPlayers, selectedPlayer,
    selectedPlayableTitle, showLanguageSelector, showPlayerSelector, showPlayerControls, activePlayers,
    rememberCurrentLanguage, rememberCurrentPlayer, resetForEntryLoad, applyLoadedEntryDefaults,
    observeSelectors, observePlayableItem,
  }
}
