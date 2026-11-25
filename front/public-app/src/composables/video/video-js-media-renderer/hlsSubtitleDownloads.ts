import {
  createWebVtt, fetchSubtitle, fetchSubtitleSegments, formatVttTime, parseVttTime,
  type SubtitleCue, type SubtitleDownload,
} from './subtitleExport'
import type { VideoJsPlayer } from './types'

interface ByteRange { offset?: number; length: number }
interface HlsResource { uri: string; byterange?: ByteRange }
interface HlsSegment extends HlsResource {
  duration: number
  discontinuity?: boolean
  gap?: boolean
  key?: unknown
  map?: HlsResource
}
interface Rendition { language?: string; forced?: boolean; resolvedUri?: string; uri?: string }
type SubtitleGroups = Record<string, Record<string, Rendition>>
interface Playlist {
  endList?: boolean
  segments?: HlsSegment[]
  mediaGroups?: { SUBTITLES?: SubtitleGroups }
}

const TIMESTAMP = '(?:[0-9]{2,}:)?[0-9]{2}:[0-9]{2}\\.[0-9]{3}'
const CUE_TIMING = new RegExp(`^(${TIMESTAMP})[ \\t]+-->[ \\t]+(${TIMESTAMP})(?:[ \\t]+(.*))?$`)
const INLINE_TIME = new RegExp(`<(${TIMESTAMP})>`, 'g')
const CLOCK_WRAP = 2 ** 33 / 90000

/**
 * Parses the master playlist or a subtitle rendition using the VHS playlist parser.
 * @param text Playlist response.
 */
async function parsePlaylist(text: string): Promise<Playlist> {
  if (!text.trimStart().startsWith('#EXTM3U')) throw new Error('Invalid HLS playlist')
  const { Parser } = await import('m3u8-parser')
  const parser = new Parser()
  parser.push(text)
  parser.end()
  return parser.manifest as Playlist
}

/**
 * Reads all subtitle renditions discovered by Video.js VHS, including forced tracks.
 * @param player Player using the HLS source.
 */
export function readHlsSubtitleSources(player: VideoJsPlayer) {
  const tech = (player as unknown as {
    tech?: (options: { IWillNotUseThisInPlugins: boolean }) => {
      vhs?: { playlists?: { main?: Playlist } }
    }
  }).tech?.({ IWillNotUseThisInPlugins: true })
  return readRenditions(tech?.vhs?.playlists?.main)
}

/**
 * Flattens HLS subtitle groups into named tracks with their rendition playlist URL.
 * @param playlist Parsed HLS master playlist.
 */
function readRenditions(playlist: Playlist | undefined) {
  const groups = playlist?.mediaGroups?.SUBTITLES ?? {}
  return Object.values(groups).flatMap((group) =>
    Object.entries(group).map(([label, track]) => ({
      link: track.resolvedUri ?? track.uri ?? '',
      lang: track.language,
      label,
      forced: track.forced === true,
      autoTranslate: false,
      hls: true,
    })),
  ).filter((entry) => entry.link)
}

/**
 * Builds subtitle download actions for a VHS or native HLS source.
 * @param renditions Renditions discovered by VHS or the master playlist.
 * @param base Master playlist URL.
 */
export function hlsSubtitleDownloads(
  renditions: ReturnType<typeof readHlsSubtitleSources>, base: string,
): SubtitleDownload[] {
  return renditions.map(({ link, lang, label }) => {
    const url = new URL(link, base).href
    return { link: url, lang, label, extension: 'vtt', assemble: (signal) => assembleRendition(url, signal) }
  })
}

/**
 * Discovers renditions for native HLS engines without VHS playlist metadata.
 * @param url Master playlist playback URL.
 * @param signal Cancellation for the discovery request.
 */
export async function discoverNativeHlsSubtitles(url: string, signal: AbortSignal): Promise<SubtitleDownload[]> {
  const response = await fetchSubtitle(url, signal)
  return hlsSubtitleDownloads(readRenditions(await parsePlaylist(await response.text())), response.url || url)
}

/**
 * Downloads one HLS subtitle resource including an optional playlist byte range.
 * @param resource Segment or initialization resource.
 * @param base Rendition playlist URL.
 * @param signal Export cancellation.
 */
async function fetchHlsResource(resource: HlsResource, base: string, signal: AbortSignal) {
  const { byterange } = resource
  const offset = byterange?.offset ?? 0
  const range = byterange ? `bytes=${offset}-${offset + byterange.length - 1}` : undefined
  return (await fetchSubtitle(new URL(resource.uri, base).href, signal, range)).arrayBuffer()
}

/**
 * Gets the local-to-transport timestamp offset from a WebVTT segment header.
 * @param header WebVTT header block.
 */
function readTimestampOffset(header: string): number | null {
  const line = header.split('\n').find((value) => value.startsWith('X-TIMESTAMP-MAP='))
  if (!line) return null
  const local = new RegExp(`LOCAL:(${TIMESTAMP})`).exec(line)?.[1]
  const mpeg = /MPEGTS:(\d+)/.exec(line)?.[1]
  if (!local || !mpeg || Number(mpeg) >= 2 ** 33) throw new Error('Invalid subtitle timestamp map')
  return Number(mpeg) / 90000 - parseVttTime(local)
}

/**
 * Merges a finite HLS WebVTT rendition into one timed document.
 * @param url Subtitle rendition URL.
 * @param signal Export cancellation and timeout.
 */
async function assembleRendition(url: string, signal: AbortSignal): Promise<Blob> {
  const response = await fetchSubtitle(url, signal)
  const text = await response.text()
  if (/^WEBVTT(?:[ \t\r\n]|$)/.test(text)) return new Blob([text], { type: 'text/vtt;charset=utf-8' })
  const playlist = await parsePlaylist(text)
  const segments = playlist.segments ?? []
  if (!playlist.endList || segments.some((segment) => segment.key || segment.gap || !Number.isFinite(segment.duration))) {
    throw new Error('The HLS subtitle rendition is incomplete or protected')
  }
  const base = response.url || url
  const buffers = await fetchSubtitleSegments(segments, (segment) => fetchHlsResource(segment, base, signal), signal)
  const decoder = new TextDecoder('utf-8', { fatal: true })
  const maps = new Map<string, Promise<string>>()
  const styles = new Set<string>()
  const cues: SubtitleCue[] = []
  let elapsed = 0
  let origin: number | undefined
  for (const [index, segment] of segments.entries()) {
    let body = decoder.decode(buffers[index]).replace(/\r\n?/g, '\n')
    if (!/^WEBVTT(?:[ \t\n]|$)/.test(body) && segment.map) {
      const key = JSON.stringify(segment.map)
      const header = maps.get(key) ?? fetchHlsResource(segment.map, base, signal).then((bytes) => decoder.decode(bytes))
      maps.set(key, header)
      const initialization = (await header).replace(/\r\n?/g, '\n')
      body = initialization.trimEnd() + '\n\n' + body
    }
    if (!/^WEBVTT(?:[ \t\n]|$)/.test(body)) throw new Error('Unsupported HLS subtitle segment')
    const [header = '', ...blocks] = body.split(/\n[ \t]*\n/)
    const mapped = readTimestampOffset(header)
    if (segment.discontinuity) origin = undefined
    let offset = 0
    if (mapped !== null) {
      origin ??= mapped - elapsed
      const start = mapped - origin
      offset = start + Math.round((elapsed - start) / CLOCK_WRAP) * CLOCK_WRAP
    }
    for (const block of blocks) {
      const lines = block.trimEnd().split('\n')
      const first = lines[0] ?? ''
      if (/^(STYLE|REGION)(?:\s|$)/.test(first)) styles.add(block.trimEnd())
      if (!first || /^NOTE(?:\s|$)/.test(first)) continue
      const index = first.includes('-->') ? 0 : 1
      const timing = CUE_TIMING.exec(lines[index] ?? '')
      if (!timing) continue
      cues.push({
        start: parseVttTime(timing[1]!) + offset,
        end: parseVttTime(timing[2]!) + offset,
        text: lines.slice(index + 1).join('\n')
          .replace(INLINE_TIME, (_, time: string) => `<${formatVttTime(parseVttTime(time) + offset)}>`),
        settings: timing[3],
        id: index === 1 ? first : undefined,
      })
    }
    elapsed += segment.duration
  }
  return createWebVtt(cues, styles)
}
