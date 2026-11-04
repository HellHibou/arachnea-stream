import type { VideoJsPlayer } from '@/composables/video/video-js-media-renderer/types'
import type { VideoJsMediaRendererProps } from '@/composables/video/video-js-media-renderer/types'
import type { VideoJsMediaRendererEmits } from '@/composables/video/video-js-media-renderer/types'
import type { ShallowRef } from 'vue'

import { useStorage } from '@/services/storage'
import {
  SKIP_CHAPTER_BUTTON_TYPES,
  type SkipChapterType,
} from '@/composables/video/video-js-media-renderer/chapters'
import {
  installAdaptiveControlBarMenuPositioning,
  installSeekOnClick,
  installTimerToggle,
  syncEpisodeAutoplayToggleControl,
  syncPrevNextVideoControls,
  type AutoplayMenuEntry,
} from '@/composables/video/video-js-media-renderer/controls'

/**
 * Builds the menu entries controlling episode autoplay and chapter autoskip.
 *
 * Advertising autoskip is skipped unconditionally and stays configured from
 * the parameters panel, so it is excluded from the menu.
 *
 * @param props Component props.
 * @param emit Component emit functions.
 * @param activePlayer Current active Video.js player instance ref.
 * @returns Entries rendered inside the episode autoplay menu.
 */
export function buildEpisodeAutoplayMenuEntries(
  props: VideoJsMediaRendererProps,
  emit: VideoJsMediaRendererEmits,
  activePlayer: ShallowRef<VideoJsPlayer | null>,
): AutoplayMenuEntry[] {
  const parameters = useStorage().getParameters()
  const entries: AutoplayMenuEntry[] = [
    {
      key: 'autoplay',
      labelKey: 'player.autoplayMenu.autoplay',
      isEnabled: props.isEpisodeAutoplayEnabled ?? false,
      onToggle: () => {
        emit('update:is-episode-autoplay-enabled', !(props.isEpisodeAutoplayEnabled ?? false))
      },
    },
  ]

  for (const chapterType of SKIP_CHAPTER_BUTTON_TYPES) {
    if (chapterType === 'ads') {
      continue
    }

    entries.push({
      key: `autoskip-${chapterType}`,
      labelKey: `settings.autoskip.${chapterType}`,
      isEnabled: parameters[`videoPlayer.autoskip.${chapterType}`].value,
      onToggle: () => {
        const parameter = useStorage().getParameters()[`videoPlayer.autoskip.${chapterType}`]
        parameter.value = !parameter.value

        const currentPlayer = activePlayer.value

        if (currentPlayer) {
          syncEpisodeAutoplayControl(currentPlayer, props, emit, activePlayer)
        }
      },
    })
  }

  return entries
}

/**
 * Updates the control bar with the current autoplay menu state.
 *
 * @param player Video.js player currently bound to the renderer.
 * @param props Component props.
 * @param emit Component emit functions.
 * @param activePlayer Current active Video.js player instance ref.
 */
export function syncEpisodeAutoplayControl(
  player: VideoJsPlayer,
  props: VideoJsMediaRendererProps,
  emit: VideoJsMediaRendererEmits,
  activePlayer: ShallowRef<VideoJsPlayer | null>,
): void {
  syncEpisodeAutoplayToggleControl(player, {
    controls: props.controls ?? false,
    showEpisodeAutoplayToggle: props.showEpisodeAutoplayToggle ?? false,
    isEpisodeAutoplayEnabled: props.isEpisodeAutoplayEnabled ?? false,
    menuEntries: buildEpisodeAutoplayMenuEntries(props, emit, activePlayer),
    onMenuOpen: () => {
      const currentPlayer = activePlayer.value

      if (currentPlayer) {
        syncEpisodeAutoplayControl(currentPlayer, props, emit, activePlayer)
      }
    },
  })
}
