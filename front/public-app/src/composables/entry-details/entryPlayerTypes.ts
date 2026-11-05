import type { Ref } from 'vue'
import type { EntryDetails, EntryPlayableItem } from '@/types/entry'

/**
 * Options accepted by the entry video player controller.
 */
export interface UseEntryVideoPlayerOptions {
  /**
   * Loaded entry details driving the active player state.
   */
  details: Ref<EntryDetails | null>
  /**
   * Playable item currently selected in the built-in player.
   */
  selectedPlayableItem: Ref<EntryPlayableItem | null>
  /** Security mode used to gate embedded trailer sources. */
  securityMode: Ref<'unsafe' | 'confirmation' | 'safe'>
  /** Whether a trailer resolver should also resolve for the decorative background. */
  useTrailerAsBackground: Ref<boolean>
}

/**
 * Display-ready language option rendered in the embedded player selector.
 */
export interface EntryPlayerLanguageOption {
  /** The stable language key used for selection. */
  key: string
  /** The human-readable language label displayed in the UI. */
  label: string
}
