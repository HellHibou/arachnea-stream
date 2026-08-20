/**
 * Runtime allowlist of trusted embedded/video sources.
 *
 * Loaded from the public `video-sources-whitelist.json` asset so operators can
 * centralize which external player and video hosts are considered safe. The
 * allowlist gates iframe (embedded player / trailer) rendering according to the
 * current security mode.
 *
 * The module keeps a stable array reference (like `services/theme.ts`) so
 * consumers holding a reference see updates without re-binding.
 */
import { resolveYoutubeEmbedUrl } from '@/services/players'

/** One allowlisted source entry loaded from the JSON asset. */
export interface VideoSourceAllowlistEntry {
  /** Human-readable name shown for maintenance. */
  label: string
  /** Allowed URL or host. A bare domain or embedded YouTube URL is recommended. */
  url: string
  /** Optional maintenance note. */
  description?: string
}

/** Runtime allowlist entries loaded from `public/video-sources-whitelist.json`. */
export const videoSourcesAllowlist: VideoSourceAllowlistEntry[] = []

/**
 * Normalized identity of a candidate or allowlisted source URL.
 */
interface SourceIdentity {
  /** Distinguishes YouTube sources for video-id-aware matching. */
  kind: 'youtube' | 'host'
  /** Lowercased hostname of the source. */
  host: string
  /** Lowercased pathname (host sources only). */
  path: string
  /** Extracted YouTube video id when the URL targets one embed/watched video. */
  videoId: string | null
}

/**
 * Checks whether a URL belongs to a YouTube host name.
 *
 * @param host Lowercased host to test.
 * @returns True when the host is a YouTube watch/embed/short-link host.
 */
function isYoutubeHost(host: string): boolean {
  return (
    host === 'youtube.com' ||
    host.endsWith('.youtube.com') ||
    host === 'youtu.be' ||
    host.endsWith('.youtu.be') ||
    host.endsWith('.youtube-nocookie.com')
  )
}

/**
 * Normalizes a media URL into a comparable source identity.
 *
 * YouTube links (watch, embed, youtu.be) are folded to their embed form so a
 * whitelisted video can be matched by its ID. Other URLs fall back to a host
 * plus path prefix match.
 *
 * @param value A raw or embedded media URL.
 * @returns Normalized source identity, or null when the URL cannot be parsed.
 */
function identifySource(value: string): SourceIdentity | null {
  try {
    const embedUrl = resolveYoutubeEmbedUrl(value)
    if (embedUrl) {
      const url = new URL(embedUrl)
      return {
        kind: 'youtube',
        host: url.hostname.toLocaleLowerCase(),
        path: url.pathname.toLocaleLowerCase(),
        videoId: url.pathname.split('/').filter(Boolean)[1] ?? null,
      }
    }

    const url = new URL(value, window.location.href)
    const host = url.hostname.toLocaleLowerCase()
    if (isYoutubeHost(host)) {
      return {
        kind: 'youtube',
        host,
        path: url.pathname.toLocaleLowerCase(),
        videoId: null,
      }
    }

    return {
      kind: 'host',
      host,
      path: url.pathname.toLocaleLowerCase(),
      videoId: null,
    }
  } catch {
    return null
  }
}
/**
 * Returns true when a candidate source URL matches a whitelist entry.
 *
 * YouTube sources match when the allowlisted URL targets the same video ID, or
 * when the allowlisted entry is a YouTube domain with no specific video (then
 * every YouTube source is allowed). Host sources match on host equality with a
 * path prefix fallback for domain-wide entries.
 *
 * @param allowlistEntry The allowlisted source entry to test against.
 * @param candidate The normalized source to check.
 * @returns True when the candidate source matches the allowlisted entry.
 */
function matchesEntry(
  allowlistEntry: VideoSourceAllowlistEntry,
  candidate: SourceIdentity,
): boolean {
  const allowed = identifySource(allowlistEntry.url)
  if (!allowed) {
    return false
  }

  if (allowed.kind === 'youtube' && candidate.kind === 'youtube') {
    return !allowed.videoId || !candidate.videoId || allowed.videoId === candidate.videoId
  }

  if (allowed.kind === 'host' && candidate.kind === 'host') {
    if (allowed.host !== candidate.host) {
      return false
    }

    if (allowed.path.trim() === '' || allowed.path === '/') {
      return true
    }

    return candidate.path.startsWith(allowed.path)
  }

  return false
}

/**
 * Whether a media URL is covered by the loaded allowlist.
 *
 * An empty allowlist reports nothing as trusted, which disables the allowlist
 * gating for video playback while leaving the rest of the security rules intact.
 *
 * @param value The resolved iframe or video source URL to check.
 * @returns True when the URL matches at least one allowlisted entry.
 */
export function isUrlAllowlisted(value: string | null): boolean {
  if (!value || videoSourcesAllowlist.length === 0) {
    return false
  }

  const candidate = identifySource(value)
  if (!candidate) {
    return false
  }

  return videoSourcesAllowlist.some((entry) => matchesEntry(entry, candidate))
}

/** Media source shape consumed by the shared security policy check. */
export interface SecurityConstrainedMediaSource {
  /** Renderer kind of the resolved source. */
  renderer: 'iframe' | 'video'
  /** Resolved source URL. */
  src?: string | null
}

/**
 * Returns whether a resolved media source may render under a security mode.
 *
 * Native video sources ('video') are always playable. Embedded iframe sources
 * play only in `unsafe` mode or when their URL is covered by the allowlist. This
 * shared policy is used by standalone surfaces such as backgrounds and banners.
 *
 * @param source The resolved media source to evaluate, or `null`/`undefined`.
 * @param mode The current security mode.
 * @returns True when the source may be rendered under the given security mode.
 */
export function isMediaSourceAllowedBySecurity(
  source: SecurityConstrainedMediaSource | null | undefined,
  mode: 'unsafe' | 'confirmation' | 'safe',
): boolean {
  if (!source) {
    return false
  }

  if (source.renderer === 'video') {
    return true
  }

  return mode === 'unsafe' || isUrlAllowlisted(source.src ?? null)
}

/**
 * Validates that a value matches the {@link VideoSourceAllowlistEntry} shape.
 *
 * @param value Value to validate.
 * @returns True when the value is a valid allowlist entry.
 */
function isVideoSourceAllowlistEntry(value: unknown): value is VideoSourceAllowlistEntry {
  if (typeof value !== 'object' || value === null) {
    return false
  }

  const record = value as Record<string, unknown>
  return typeof record.label === 'string' && typeof record.url === 'string'
}

/**
 * Loads allowlist entries from the public `video-sources-whitelist.json` asset.
 *
 * Call this once during app startup, before player surfaces need to enforce the
 * allowlist. Subsequent calls are safe but re-fetch the asset.
 *
 * When the fetch fails or the payload is invalid, the allowlist is left empty so
 * the app still renders and falls back to the confirmation flow.
 *
 * @returns Promise resolving to the loaded allowlist entries.
 */
export async function loadVideoSourceAllowlist(): Promise<VideoSourceAllowlistEntry[]> {
  try {
    const response = await fetch('/video-sources-whitelist.json')
    if (!response.ok) {
      throw new Error(`Unable to load video source allowlist (${response.status}).`)
    }

    const payload: unknown = await response.json()
    const records =
      typeof payload === 'object' && payload !== null
        ? ((payload as Record<string, unknown>).sources ?? payload)
        : payload

    const candidates = Array.isArray(records) ? records : []
    videoSourcesAllowlist.length = 0
    for (const item of candidates) {
      if (isVideoSourceAllowlistEntry(item)) {
        videoSourcesAllowlist.push(item)
      }
    }
  } catch {
    videoSourcesAllowlist.length = 0
  }

  return videoSourcesAllowlist
}