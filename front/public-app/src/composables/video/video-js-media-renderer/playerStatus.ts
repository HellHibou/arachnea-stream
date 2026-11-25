import { shallowRef, type ShallowRef } from 'vue'

/** Producers that can show a transient message above the player controls. */
export type PlayerStatusSource = 'translation' | 'download'

/** One transient player status message. */
export interface PlayerStatusMessage {
  /** Localized text displayed in the player. */
  text: string
  /** Whether a spinner should precede the text. */
  spinner: boolean
}

/**
 * Creates the single status channel shared by the subtitle translation and the download menu.
 *
 * Translation takes precedence over downloads because it is usually a longer
 * operation; a download message reappears once the translation message is cleared.
 *
 * @returns Reactive current message and per-source publish/clear functions.
 */
export function createPlayerStatus() {
  const messages = new Map<PlayerStatusSource, PlayerStatusMessage>()
  const current = shallowRef<PlayerStatusMessage | null>(null) as ShallowRef<PlayerStatusMessage | null>

  /** Recomputes the visible message from active producers. */
  function refresh() {
    current.value = messages.get('translation') ?? messages.get('download') ?? null
  }

  /**
   * Publishes or replaces the message owned by one producer.
   * @param source Producer publishing the message.
   * @param text Localized message to show.
   * @param spinner Whether the message needs a spinner.
   */
  function set(source: PlayerStatusSource, text: string, spinner = false): void {
    messages.set(source, { text, spinner })
    refresh()
  }

  /**
   * Removes the message owned by one producer.
   * @param source Producer whose message should disappear.
   */
  function clear(source: PlayerStatusSource): void {
    if (messages.delete(source)) refresh()
  }

  return { current, set, clear }
}
