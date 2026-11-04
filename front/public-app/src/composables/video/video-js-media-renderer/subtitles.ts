import type { ResolvedVideoMediaSource } from '@/services/players'
import type {
  VideoJsPlayer,
  VideoJsRemoteTextTrackHandle,
} from '@/composables/video/video-js-media-renderer/types'

/**
 * Derives a display label from a WebVTT file URL when the resolver did not provide one.
 *
 * @param link Subtitle track URL.
 * @returns Decoded filename without its `.vtt` extension, or undefined when unavailable.
 */
export function getSubtitleFileLabel(link: string): string | undefined {
  try {
    const url = new URL(link, window.location.origin)
    const pathSegments = url.pathname.split('/').filter(Boolean)
    const filename = pathSegments[pathSegments.length - 1]

    if (!filename) {
      return undefined
    }

    const label = decodeURIComponent(filename).replace(/\.vtt$/i, '').trim()
    return label || undefined
  } catch {
    return undefined
  }
}

/**
 * Checks whether two subtitle lists identify the same remote text tracks.
 *
 * @param left First subtitle list.
 * @param right Second subtitle list.
 * @returns True when the lists have identical track links and display metadata in the same order.
 */
export function areSubtitlesEqual(
  left: ResolvedVideoMediaSource['subtitles'],
  right: ResolvedVideoMediaSource['subtitles'],
): boolean {
  return left.length === right.length && left.every((subtitle, index) => {
    const other = right[index]
    return other !== undefined &&
      subtitle.link === other.link &&
      subtitle.lang === other.lang &&
      subtitle.label === other.label
  })
}

/**
 * Removes all remote subtitle tracks created by this renderer.
 *
 * @param player Video.js player that owns the tracks.
 * @param remoteSubtitleTracks Remote subtitle tracks explicitly owned by the active renderer.
 * @returns An empty array, ready to replace the caller's `remoteSubtitleTracks` variable.
 */
export function clearRemoteSubtitleTracks(
  player: VideoJsPlayer,
  remoteSubtitleTracks: VideoJsRemoteTextTrackHandle[],
): VideoJsRemoteTextTrackHandle[] {
  for (const track of remoteSubtitleTracks) {
    player.removeRemoteTextTrack?.(track)
  }

  return []
}

/**
 * Adds the resolver-provided subtitle tracks to the current Video.js player.
 *
 * @param player Video.js player receiving the remote tracks.
 * @param source Media source containing normalized subtitle metadata.
 * @param remoteSubtitleTracks Remote subtitle tracks explicitly owned by the active renderer.
 */
export function addRemoteSubtitleTracks(
  player: VideoJsPlayer,
  source: ResolvedVideoMediaSource,
  remoteSubtitleTracks: VideoJsRemoteTextTrackHandle[],
): void {
  for (const subtitle of source.subtitles) {
    const label = subtitle.label ?? subtitle.lang ?? getSubtitleFileLabel(subtitle.link)

    if (!label) {
      continue
    }

    const track = player.addRemoteTextTrack?.(
      {
        kind: 'subtitles',
        src: subtitle.link,
        label,
        ...(subtitle.lang ? { srclang: subtitle.lang } : {}),
      },
      true,
    )

    if (track) {
      remoteSubtitleTracks.push(track)
    }
  }
}
