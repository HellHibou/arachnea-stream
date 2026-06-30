export interface ResolvedIframeMediaSource {
  renderer: 'iframe'
  src: string
}

export interface ResolvedVideoSpriteThumbnails {
  url: string
  width: number
  height: number
  columns: number
  interval: number
}

export interface ResolvedVideoMediaSource {
  renderer: 'video'
  src: string
  mimeType: string | null
  transport: 'file' | 'hls' | 'dash'
  licenseUrl: string | null
  licenseHeaders: Record<string, string>
  storyboard: ResolvedVideoSpriteThumbnails | null
}

export type ResolvedPlayerMediaSource =
  | ResolvedIframeMediaSource
  | ResolvedVideoMediaSource

export const HLS_MIME_TYPE = 'application/vnd.apple.mpegurl'
export const DASH_MIME_TYPE = 'application/dash+xml'

interface ResolvedNativeVideoAsset {
  src: string
  mimeType: string | null
  transport: ResolvedVideoMediaSource['transport']
}

function extractEmbeddedUrl(value: string, pattern: RegExp): string | null {
  const match = value.match(pattern)

  if (!match) {
    return null
  }

  return match[1] ?? match[0] ?? null
}

function extractAbsoluteUrl(value: string): string | null {
  return extractEmbeddedUrl(value, /(https?:\/\/[^"'\\\s<>]+)/i)
}

function resolveProtocolRelativeUrl(value: string): string | null {
  if (value.startsWith('//')) {
    return 'https:' + value
  }
  return null
}

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
 * @param streamUrl Resolved stream URL returned by the backend.
 * @param manifestType Stream manifest type returned by the backend.
 * @param licenseUrl Optional DRM license URL returned by the backend.
 * @param licenseHeaders Optional DRM license headers returned by the backend.
 * @returns Resolved player source describing whether to render DASH or a native video asset.
 */
export function resolveBackendStreamMediaSource(
  streamUrl: string | null,
  manifestType: string | null,
  licenseUrl: string | null = null,
  licenseHeaders: Record<string, string> = {},
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
    }
  }

  return null
}
