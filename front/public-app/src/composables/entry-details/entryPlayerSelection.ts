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
 * Orders native streams, unresolved candidates and confirmed iframes in that order.
 * @param players Players available for one language.
 * @param playerRenderers Renderer kinds discovered during stream resolution.
 * @returns Sorted copy of the candidates.
 */
function sortEntryPlayers(players: EntryPlayer[], playerRenderers: ReadonlyMap<string, 'video' | 'iframe'>): EntryPlayer[] {
  /** @param player Candidate whose known renderer takes precedence over its discovery hint. */
  const rank = (player: EntryPlayer): number => {
    const renderer = playerRenderers.get(player.id)
    if (renderer === 'iframe') return 3
    if (renderer === 'video' || player.streams.length > 0) return 0
    return player.isEmbed ? 2 : 1
  }
  return [...players].sort((left, right) => rank(left) - rank(right))
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

  /** Iframe classifications are retained only for the current entry. */
  const playerRenderers = shallowRef<ReadonlyMap<string, 'video' | 'iframe'>>(new Map())
  const failedPlayerIds = new Set<string>()
  let languagePlayerName: string | null = null
  let prioritizedPlayerId: string | null = null
  let explicitPlayerId: string | null = null

  /**
   * Exposes the embedded players returned by the backend entry payload.
   *
   * @returns Array of all players for the entry.
   */
  const availablePlayers = computed<EntryPlayer[]>(() => options.details.value?.players.entries ?? [])

  /**
   * Exposes the players that should drive the built-in player.
   * Keeps deferred episode lists empty until loaded, otherwise falls back to entry players.
   *
   * @returns Array of players relevant to the current playable context.
   */
  const activePlayers = computed<EntryPlayer[]>(() => {
    const item = options.selectedPlayableItem.value
    if (item && (item.players.entries.length > 0 || item.players.link)) return item.players.entries
    return availablePlayers.value
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
      if (value !== activeLanguageKey.value) {
        languagePlayerName = selectedPlayer.value?.name?.trim().toLowerCase() ?? null
      }
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
      return sortEntryPlayers(activePlayers.value, playerRenderers.value)
    }

    const currentLanguageKey = resolvedLanguageKey.value
    return sortEntryPlayers(
      activePlayers.value.filter((player) => getPlayerLanguageKey(player) === currentLanguageKey),
      playerRenderers.value,
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

  /** Pins an explicit server selection so its errors do not trigger server fallback. */
  function rememberCurrentPlayer(): void {
    failedPlayerIds.clear()
    prioritizedPlayerId = selectedPlayer.value?.id ?? null
    explicitPlayerId = prioritizedPlayerId
  }

  /**
   * Advances after one failed server, trying every remaining server at most once.
   * @returns Whether a remaining server was selected; explicit server choices never advance.
   */
  function selectNextPlayer(): boolean {
    const current = selectedPlayer.value
    if (!current || current.id === explicitPlayerId) return false
    prioritizedPlayerId = null
    failedPlayerIds.add(current.id)
    const next = filteredPlayers.value.find((player) => !failedPlayerIds.has(player.id))
    if (!next) return false
    activePlayerId.value = next.id
    return true
  }

  /**
   * Defers an iframe until native and unresolved candidates have been tried.
   * @param player Player whose renderer was resolved.
   * @param renderer Renderer returned by the stream resolver.
   * @returns Whether this source should be displayed immediately.
   */
  function acceptResolvedPlayer(player: EntryPlayer, renderer: 'video' | 'iframe'): boolean {
    playerRenderers.value = new Map([...playerRenderers.value, [player.id, renderer]])
    if (renderer !== 'iframe' || player.id === prioritizedPlayerId) return true
    const next = filteredPlayers.value.find((candidate) => !failedPlayerIds.has(candidate.id))
    if (!next || next.id === player.id) return true
    activePlayerId.value = next.id
    return false
  }

  /** Clears entry and item language choices before loading another entry. */
  function resetForEntryLoad() {
    preferredLanguageKey.value = null
    languagePlayerName = null
    prioritizedPlayerId = null
    explicitPlayerId = null
    failedPlayerIds.clear()
    playerRenderers.value = new Map()
    selectedLanguageKey.value = null
    selectedPlayerId.value = null
    clearSelectedPlayablePlayerState()
  }

  /** Starts a freshly loaded entry without a previous entry's language choice. */
  function applyLoadedEntryDefaults() {
    resetForEntryLoad()
  }

  /** Reconciles language and player choices after episode defaults are initialized. */
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
    watch(
      [activePlayers, resolvedLanguageKey],
      () => {
        failedPlayerIds.clear()
        explicitPlayerId = null
        const players = filteredPlayers.value
        const preferred = languagePlayerName
          ? players.find((player) => player.name?.trim().toLowerCase() === languagePlayerName)
          : null
        languagePlayerName = null
        prioritizedPlayerId = preferred?.id ?? null
        activePlayerId.value = preferred?.id ?? players[0]?.id ?? null
      },
      { immediate: true },
    )
  }

  /** Restores the language across episodes while starting at the first sorted server. */
  function observePlayableItem() {
    watch(
      () => options.selectedPlayableItem.value?.id ?? null,
      (nextPlayableItemId, previousPlayableItemId) => {
        clearSelectedPlayablePlayerState()
        languagePlayerName = null
        prioritizedPlayerId = null
        explicitPlayerId = null
        failedPlayerIds.clear()
        if (!nextPlayableItemId || nextPlayableItemId === previousPlayableItemId) return
        const players = options.selectedPlayableItem.value?.players.entries ?? []
        const languages = buildEntryPlayerLanguageOptions(players)
        selectedPlayableLanguageKey.value = languages.some((language) => language.key === preferredLanguageKey.value)
          ? preferredLanguageKey.value
          : languages[0]?.key ?? null
        selectedPlayablePlayerId.value = sortEntryPlayers(
          players.filter((player) => getPlayerLanguageKey(player) === selectedPlayableLanguageKey.value),
          playerRenderers.value,
        )[0]?.id ?? null
        options.activateMedia()
      },
    )
  }

  return {
    activeLanguageKey, activePlayerId, availableLanguages, filteredPlayers, selectedPlayer,
    selectedPlayableTitle, showLanguageSelector, showPlayerSelector, showPlayerControls, activePlayers,
    rememberCurrentLanguage, rememberCurrentPlayer, resetForEntryLoad, applyLoadedEntryDefaults,
    observeSelectors, observePlayableItem, selectNextPlayer, acceptResolvedPlayer,
  }
}
