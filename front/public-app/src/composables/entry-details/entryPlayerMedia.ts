import type { EntryPlayer, ResolvedPlayerSubtitle } from '@/types/entry'
import type { ResolvedPlayerMediaSource, ResolvedVideoChapter, ResolvedVideoSpriteThumbnails } from '@/services/players'

/**
 * Copies one normalized storyboard into the media-source format consumed by Video.js.
 *
 * @param player Player currently selected in the UI.
 * @returns Sprite thumbnail configuration when available.
 */
export function resolvePlayerStoryboard(player: EntryPlayer | null): ResolvedVideoSpriteThumbnails | null {
  if (!player?.storyboard) {
    return null
  }

  return { ...player.storyboard }
}

/**
 * Copies normalized player chapters into the media-source format consumed by Video.js.
 *
 * @param player Player currently selected in the UI.
 * @returns Chapter list when available.
 */
export function resolvePlayerChapters(player: EntryPlayer | null): ResolvedVideoChapter[] | null {
  return player?.chapters ?? null
}

/**
 * Copies normalized player subtitles into the media-source format consumed by Video.js.
 *
 * @param player Player currently selected in the UI.
 * @returns Subtitle tracks carried by the player.
 */
export function resolvePlayerSubtitles(player: EntryPlayer | null): ResolvedPlayerSubtitle[] {
  return player?.subtitles ?? []
}

/**
 * Enriches one resolved media source with storyboard and subtitle metadata when it renders through Video.js.
 *
 * @param mediaSource Resolved player media source.
 * @param storyboard Sprite thumbnail configuration carried by the selected player.
 * @param chapters Chapter list carried by the selected player.
 * @param subtitles Subtitle tracks to attach, or null to keep the media-source tracks.
 * @returns Video source including storyboard metadata when applicable.
 */
export function attachStoryboardToVideoSource(
  mediaSource: ResolvedPlayerMediaSource | null,
  storyboard: ResolvedVideoSpriteThumbnails | null,
  chapters: ResolvedVideoChapter[] | null = null,
  subtitles: ResolvedPlayerSubtitle[] | null = null,
): ResolvedPlayerMediaSource | null {
  if (!mediaSource || mediaSource.renderer !== 'video') {
    return mediaSource
  }

  return {
    ...mediaSource,
    storyboard,
    chapters: chapters ?? mediaSource.chapters,
    subtitles: subtitles && subtitles.length > 0 ? subtitles : mediaSource.subtitles,
  }
}
