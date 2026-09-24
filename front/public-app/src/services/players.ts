import type { ResolvedPlayerSubtitle } from '@/types/entry'

/**
 * Resolved media source for iframe-based player rendering.
 */
export interface ResolvedIframeMediaSource {
  /** The renderer type for this source. */
  renderer: 'iframe'
  /** The URL to embed in the iframe. */
  src: string
}

/**
 * Sprite thumbnail metadata for video storyboards.
 */
export interface ResolvedVideoSpriteThumbnails {
  /** The URL to the sprite thumbnail image. */
  url: string
  /** The width of each thumbnail in the sprite, inferred from the image when omitted. */
  width?: number
  /** The height of each thumbnail in the sprite, inferred from the image when omitted. */
  height?: number
  /** The number of columns in the sprite image. */
  columns: number
  /** The number of thumbnail rows in each sprite image. */
  rows: number
  /** Index used for the first sprite image in a sequential URL template. */
  firstPageIndex: number
  /** The time interval between thumbnails in seconds, or `null` when derived from video duration. */
  interval: number | null
}

/**
 * A single chapter entry within a resolved player stream.
 */
export interface ResolvedVideoChapter {
  /** Start time of the chapter in seconds. */
  start: number
  /** End time of the chapter in seconds. */
  end: number
  /** Display title of the chapter. */
  title: string
  /** Type discriminator for the chapter (e.g. "chapter"). */
  type: string
}

/**
 * Resolved media source for native video player rendering.
 */
export interface ResolvedVideoMediaSource {
  /** The renderer type for this source. */
  renderer: 'video'
  /** The URL to the video source or manifest. */
  src: string
  /** The MIME type of the video source. */
  mimeType: string | null
  /** The transport protocol used for the video. */
  transport: 'file' | 'hls' | 'dash'
  /** The URL to the DRM license server, or null for unprotected content. */
  licenseUrl: string | null
  /** Headers to include when requesting the license. */
  licenseHeaders: Record<string, string>
  /** The sprite storyboard metadata for this source. */
  storyboard: ResolvedVideoSpriteThumbnails | null
  /** Optional WebVTT URL, preferred over sprite thumbnail metadata for previews. */
  storyboardVttUrl?: string | null
  /** Optional ordered list of chapters extracted from the player metadata. */
  chapters: ResolvedVideoChapter[] | null
  /** Subtitle tracks available for the media source. */
  subtitles: ResolvedPlayerSubtitle[]
}

/** Union type for all resolved player media sources. */
export type ResolvedPlayerMediaSource =
  | ResolvedIframeMediaSource
  | ResolvedVideoMediaSource

/** MIME type for HLS streaming manifests. */
export const HLS_MIME_TYPE = 'application/vnd.apple.mpegurl'
/** MIME type for DASH streaming manifests. */
export const DASH_MIME_TYPE = 'application/dash+xml'

/**
 * Internal representation of a native video asset extracted from a URL.
 */
interface ResolvedNativeVideoAsset {
  /** The URL to the video source. */
  src: string
  /** The MIME type of the video source. */
  mimeType: string | null
  /** The transport protocol used for the video. */
  transport: ResolvedVideoMediaSource['transport']
}

/**
 * Backend stream data required to build a playable media source.
 */
export interface BackendStreamMediaSourceOptions {
  /** Resolved stream URL returned by the backend. */
  streamUrl: string | null
  /** Stream manifest type returned by the backend. */
  manifestType: string | null
  /** Optional DRM license URL returned by the backend. */
  licenseUrl?: string | null
  /** Optional DRM license headers returned by the backend. */
  licenseHeaders?: Record<string, string>
  /** Optional storyboard WebVTT URL returned by the backend. */
  storyboardVttUrl?: string | null
  /** Optional chapters extracted from the player metadata. */
  chapters?: ResolvedVideoChapter[] | null
  /** Optional subtitle tracks extracted from the player metadata. */
  subtitles?: ResolvedPlayerSubtitle[]
}

/**
 * Extracts a URL matching the given pattern from a string.
 *
 * @param value - The string to search for a URL.
 * @param pattern - Regular expression to match the URL.
 * @returns The matched URL or null if not found.
 */
function extractEmbeddedUrl(value: string, pattern: RegExp): string | null {
  const match = value.match(pattern)

  if (!match) {
    return null
  }

  return match[1] ?? match[0] ?? null
}

/**
 * Extracts an absolute HTTP/HTTPS URL from a string.
 *
 * @param value - The string to extract a URL from.
 * @returns The absolute URL or null if not found.
 */
function extractAbsoluteUrl(value: string): string | null {
  return extractEmbeddedUrl(value, /(https?:\/\/[^"'\\\s<>]+)/i)
}

/**
 * Resolves a protocol-relative URL to an absolute HTTPS URL.
 *
 * @param value - The URL string to resolve.
 * @returns The absolute HTTPS URL or null if not protocol-relative.
 */
function resolveProtocolRelativeUrl(value: string): string | null {
  if (value.startsWith('//')) {
    return 'https:' + value
  }
  return null
}

/**
 * Resolves the MIME type for a video file based on its pathname.
 *
 * @param pathname - The pathname or URL path to check.
 * @returns The MIME type for supported video formats, or null if not recognized.
 */
function resolveNativeVideoMimeType(pathname: string): string | null {
  if (pathname.endsWith('.mp4')) {
    return 'video/mp4'
  }

  if (pathname.endsWith('.webm')) {
    return 'video/webm'
  }

  if (pathname.endsWith('.ogg')) {
    return 'video/ogg'
  }

  if (pathname.endsWith('.m3u8')) {
    return HLS_MIME_TYPE
  }

  if (pathname.endsWith('.mpd')) {
    return DASH_MIME_TYPE
  }

  return null
}

/**
 * Converts a parsed URL into a native video source when its path targets a supported asset.
 *
 * @param url Parsed candidate media URL.
 * @returns Native video asset metadata when the URL points to a supported video or manifest.
 */
function createNativeVideoAssetFromUrl(url: URL): ResolvedNativeVideoAsset | null {
  const pathname = url.pathname.toLocaleLowerCase()
  const mimeType = resolveNativeVideoMimeType(pathname)

  if (!mimeType) {
    return null
  }

  return {
    src: url.toString(),
    mimeType,
    transport:
      mimeType === HLS_MIME_TYPE
        ? 'hls'
        : mimeType === DASH_MIME_TYPE
          ? 'dash'
          : 'file',
  }
}

/**
 * Indicates whether the full raw value should be resolved before looking for embedded URLs.
 *
 * @param value Trimmed media value returned by the backend.
 * @returns True when the value looks like a URL, path, or plain media filename.
 */
function shouldResolveNativeVideoValue(value: string): boolean {
  return (
    /^[a-z][a-z\d+.-]*:\/\//i.test(value) ||
    value.startsWith('/') ||
    value.startsWith('./') ||
    value.startsWith('../') ||
    (/^[^\s"'<>]+$/i.test(value) && /\.(?:mp4|webm|ogg|m3u8|mpd)(?:[?#].*)?$/i.test(value))
  )
}

/**
 * Resolves a raw media value into a native video asset.
 *
 * @param value - The media URL or path to resolve.
 * @returns Resolved native video asset or null if not a supported format.
 */
function resolveNativeVideoAsset(value: string | null): ResolvedNativeVideoAsset | null {
  if (!value) {
    return null
  }

  const normalizedValue = value.trim()

  if (!normalizedValue) {
    return null
  }

  try {
    const candidateUrl = resolveProtocolRelativeUrl(normalizedValue) ?? normalizedValue

    if (shouldResolveNativeVideoValue(candidateUrl)) {
      const nativeVideoAsset = createNativeVideoAssetFromUrl(
        new URL(candidateUrl, window.location.href),
      )

      if (nativeVideoAsset) {
        return nativeVideoAsset
      }
    }
  } catch {
    // Continue with the embedded URL fallback below.
  }

  try {
    const embeddedUrl = extractAbsoluteUrl(normalizedValue)

    if (!embeddedUrl) {
      return null
    }

    return createNativeVideoAssetFromUrl(new URL(embeddedUrl))
  } catch {
    return null
  }
}

/**
 * Resolves a URL string to an absolute URL.
 *
 * @param value - The URL string to resolve.
 * @returns The absolute URL or null if resolution fails.
 */
function resolveAbsoluteUrl(value: string | null): string | null {
  if (!value) {
    return null
  }

  try {
    return new URL(value, window.location.href).toString()
  } catch {
    const protocolRelative = resolveProtocolRelativeUrl(value)
    if (protocolRelative) {
      return new URL(protocolRelative).toString()
    }
    return extractAbsoluteUrl(value)
  }
}

/**
 * Normalizes a YouTube link into an embeddable URL.
 *
 * @param value Raw YouTube URL returned by the backend.
 * @returns Safe embed URL when the value targets YouTube.
 */
export function resolveYoutubeEmbedUrl(value: string | null): string | null {
  if (!value) {
    return null
  }

  try {
    const candidateUrl = extractEmbeddedUrl(
      value,
      /(https?:\/\/(?:www\.)?(?:youtube(?:-nocookie)?\.com\/[^"'\s<>]+|youtu\.be\/[^"'\s<>]+))/i,
    ) ?? extractAbsoluteUrl(value) ?? resolveProtocolRelativeUrl(value) ?? value
    const url = new URL(candidateUrl)
    const host = url.hostname.toLocaleLowerCase()

    if (host === 'youtu.be') {
      const videoId = url.pathname.split('/').filter(Boolean)[0]
      return videoId ? `https://www.youtube.com/embed/${videoId}` : null
    }

    if (host.endsWith('youtube.com') || host.endsWith('youtube-nocookie.com')) {
      if (url.pathname.startsWith('/embed/')) {
        return url.toString()
      }

      if (url.pathname === '/watch') {
        const videoId = url.searchParams.get('v')
        return videoId ? `https://www.youtube.com/embed/${videoId}` : null
      }
    }
  } catch {
    return null
  }

  return null
}

/**
 * Normalizes a direct video asset URL returned by the backend.
 *
 * @param value Raw video URL returned by the backend.
 * @returns Safe absolute video URL when the value targets a supported asset.
 */
export function resolveNativeVideoUrl(value: string | null): string | null {
  return resolveNativeVideoAsset(value)?.src ?? null
}

/**
 * Resolves one raw media value into a dedicated iframe renderer.
 *
 * @param value Raw media URL returned by the backend.
 * @returns Resolved iframe source when the value can be loaded in an embedded player.
 */
export function resolveIframeMediaSource(value: string | null): ResolvedIframeMediaSource | null {
  if (!value) {
    return null
  }

  const embedUrl = resolveYoutubeEmbedUrl(value)
  if (embedUrl) {
    return {
      renderer: 'iframe',
      src: embedUrl,
    }
  }

  const absoluteUrl = resolveAbsoluteUrl(value)
  if (!absoluteUrl) {
    return null
  }

  return {
    renderer: 'iframe',
    src: absoluteUrl,
  }
}

/**
 * Resolves one raw media value into the renderer used by the embedded player.
 *
 * @param value Raw media URL returned by the backend.
 * @returns Resolved player source describing whether to render an iframe or a video.
 */
export function resolvePlayerMediaSource(value: string | null): ResolvedPlayerMediaSource | null {
  if (!value) {
    return null
  }

  const embedUrl = resolveYoutubeEmbedUrl(value)
  if (embedUrl) {
    return {
      renderer: 'iframe',
      src: embedUrl,
    }
  }

  const nativeVideoAsset = resolveNativeVideoAsset(value)
  if (nativeVideoAsset) {
    return {
      renderer: 'video',
      src: nativeVideoAsset.src,
      mimeType: nativeVideoAsset.mimeType,
      transport: nativeVideoAsset.transport,
      licenseUrl: null,
      licenseHeaders: {},
      storyboard: null,
      chapters: null,
      subtitles: [],
    }
  }

  if (value.startsWith('http://') || value.startsWith('https://') || value.startsWith('//')) {
    const resolvedUrl = extractAbsoluteUrl(value) ?? resolveProtocolRelativeUrl(value) ?? value
    return {
      renderer: 'iframe',
      src: resolvedUrl,
    }
  }

  return null
}

/**
 * Resolves one backend-provided manifest payload into the renderer used by the embedded player.
 *
 * @param options Backend stream data returned by the resolver.
 * @returns Resolved player source describing whether to render DASH or a native video asset.
 */
export function resolveBackendStreamMediaSource(
  {
    streamUrl,
    manifestType,
    licenseUrl = null,
    licenseHeaders = {},
    storyboardVttUrl = null,
    chapters = null,
    subtitles = [],
  }: BackendStreamMediaSourceOptions,
): ResolvedPlayerMediaSource | null {
  const normalizedManifestType = manifestType?.trim().toLocaleLowerCase() ?? null
  const normalizedStreamUrl = resolveAbsoluteUrl(streamUrl)

  if (!normalizedStreamUrl) {
    return null
  }

  if (normalizedManifestType === 'mpd' || normalizedManifestType === 'dash') {
    return {
      renderer: 'video',
      src: normalizedStreamUrl,
      mimeType: DASH_MIME_TYPE,
      transport: 'dash',
      licenseUrl: resolveAbsoluteUrl(licenseUrl),
      licenseHeaders,
      storyboard: null,
      chapters,
      storyboardVttUrl: resolveAbsoluteUrl(storyboardVttUrl),
      subtitles: subtitles.flatMap((subtitle) => {
        const link = resolveAbsoluteUrl(subtitle.link)
        return link ? [{ ...subtitle, link }] : []
      }),
    }
  }

  if (normalizedManifestType === 'm3u8' || normalizedManifestType === 'hls') {
    return {
      renderer: 'video',
      src: normalizedStreamUrl,
      mimeType: HLS_MIME_TYPE,
      transport: 'hls',
      licenseUrl: null,
      licenseHeaders: {},
      storyboard: null,
      chapters,
      storyboardVttUrl: resolveAbsoluteUrl(storyboardVttUrl),
      subtitles: subtitles.flatMap((subtitle) => {
        const link = resolveAbsoluteUrl(subtitle.link)
        return link ? [{ ...subtitle, link }] : []
      }),
    }
  }

  if (normalizedManifestType === 'mp4') {
    return {
      renderer: 'video',
      src: normalizedStreamUrl,
      mimeType: 'video/mp4',
      transport: 'file',
      licenseUrl: null,
      licenseHeaders: {},
      storyboard: null,
      chapters,
      storyboardVttUrl: resolveAbsoluteUrl(storyboardVttUrl),
      subtitles: subtitles.flatMap((subtitle) => {
        const link = resolveAbsoluteUrl(subtitle.link)
        return link ? [{ ...subtitle, link }] : []
      }),
    }
  }

  return resolvePlayerMediaSource(streamUrl)
}

/**
 * Normalizes a YouTube link into a background-safe embed URL.
 *
 * The returned URL autoplays muted, loops, hides controls, and disables
 * interactions so it can be used as a decorative full-page background.
 *
 * @param value Raw or embedded YouTube URL.
 * @returns Safe background embed URL when the value targets YouTube.
 */
export function resolveYoutubeBackgroundEmbedUrl(value: string | null): string | null {
  const embedUrl = resolveYoutubeEmbedUrl(value)

  if (!embedUrl) {
    return null
  }

  try {
    const url = new URL(embedUrl)
    const videoId = url.pathname.split('/').filter(Boolean)[1]

    url.searchParams.set('autoplay', '1')
    url.searchParams.set('controls', '0')
    url.searchParams.set('disablekb', '1')
    url.searchParams.set('fs', '0')
    url.searchParams.set('loop', '1')
    url.searchParams.set('modestbranding', '1')
    url.searchParams.set('mute', '1')
    url.searchParams.set('playsinline', '1')
    url.searchParams.set('rel', '0')

    if (videoId) {
      url.searchParams.set('playlist', videoId)
    }

    return url.toString()
  } catch {
    return null
  }
}

/**
 * Resolves one raw media value into the renderer used by decorative background videos.
 *
 * @param value Raw media URL returned by the backend.
 * @returns Resolved player source describing whether to render an iframe or a video.
 */
export function resolveBackgroundMediaSource(
  value: string | null,
): ResolvedPlayerMediaSource | null {
  if (!value) {
    return null
  }

  const embedUrl = resolveYoutubeBackgroundEmbedUrl(value)
  if (embedUrl) {
    return {
      renderer: 'iframe',
      src: embedUrl,
    }
  }

  const nativeVideoAsset = resolveNativeVideoAsset(value)
  if (nativeVideoAsset) {
    return {
      renderer: 'video',
      src: nativeVideoAsset.src,
      mimeType: nativeVideoAsset.mimeType,
      transport: nativeVideoAsset.transport,
      licenseUrl: null,
      licenseHeaders: {},
      storyboard: null,
      chapters: null,
      subtitles: [],
    }
  }

  return null
}
