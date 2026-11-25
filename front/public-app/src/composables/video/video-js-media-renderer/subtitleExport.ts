export interface SubtitleDownload {
  link: string
  lang?: string
  label?: string
  extension: 'vtt' | 'ttml'
  assemble?: (signal: AbortSignal) => Promise<Blob>
}

export interface SubtitleCue {
  start: number
  end: number
  text: string
  settings?: string
  id?: string
}

export const MAX_SUBTITLE_SEGMENTS = 5000
const MAX_SUBTITLE_BYTES = 32 * 1024 * 1024
const SEGMENT_BATCH_SIZE = 6

/**
 * Fetches a subtitle resource within the player's browser and proxy context.
 * @param url Subtitle URL.
 * @param signal Export cancellation and timeout.
 * @param range Optional byte range.
 */
export async function fetchSubtitle(url: string, signal: AbortSignal, range?: string) {
  const target = new URL(url, document.baseURI)
  if (!['http:', 'https:'].includes(target.protocol) && target.protocol !== location.protocol) {
    throw new Error('Unsupported subtitle URL')
  }
  const response = await fetch(target, { signal, headers: range ? { Range: range } : undefined })
  if (!response.ok || (range && response.status !== 206)) throw new Error('Subtitle resource unavailable')
  if (Number(response.headers.get('content-length')) > MAX_SUBTITLE_BYTES) {
    await response.body?.cancel()
    throw new Error('Subtitle resource is too large')
  }
  return response
}

/**
 * Retrieves all resources in order with bounded concurrency and total size.
 * @param segments Ordered requests.
 * @param retrieve Operation used for a request.
 * @param signal Export cancellation and timeout.
 */
export async function fetchSubtitleSegments<T>(
  segments: T[],
  retrieve: (segment: T) => Promise<ArrayBuffer>,
  signal: AbortSignal,
): Promise<ArrayBuffer[]> {
  if (!segments.length || segments.length > MAX_SUBTITLE_SEGMENTS) throw new Error('Unsupported subtitle segment count')
  const results: ArrayBuffer[] = []
  let size = 0
  for (let start = 0; start < segments.length; start += SEGMENT_BATCH_SIZE) {
    signal.throwIfAborted()
    const batch = await Promise.all(segments.slice(start, start + SEGMENT_BATCH_SIZE).map(retrieve))
    for (const bytes of batch) {
      size += bytes.byteLength
      if (size > MAX_SUBTITLE_BYTES) throw new Error('Subtitle export is too large')
      results.push(bytes)
    }
  }
  return results
}

/**
 * Formats a WebVTT timestamp from seconds.
 * @param seconds Position on the presentation timeline.
 */
export function formatVttTime(seconds: number): string {
  const ms = Math.round(Math.max(0, seconds) * 1000)
  const pad = (value: number, width = 2) => String(value).padStart(width, '0')
  return `${pad(Math.floor(ms / 3600000))}:${pad(Math.floor(ms / 60000) % 60)}:${pad(Math.floor(ms / 1000) % 60)}.${pad(ms % 1000, 3)}`
}

/**
 * Parses a WebVTT timestamp into seconds.
 * @param time Timestamp token from a WebVTT document.
 */
export function parseVttTime(time: string): number {
  return time.split(':').reduce((seconds, token) => seconds * 60 + Number(token), 0)
}

/**
 * Writes deduplicated, sorted cues to a complete WebVTT document.
 * @param cues Timed subtitles in presentation seconds.
 * @param blocks Optional WebVTT STYLE and REGION blocks.
 */
export function createWebVtt(cues: SubtitleCue[], blocks: Iterable<string> = []): Blob {
  const entries: SubtitleCue[] = []
  const lastByPayload = new Map<string, SubtitleCue>()
  for (const cue of cues.sort((a, b) => a.start - b.start || a.end - b.end)) {
    if (cue.end <= cue.start || cue.end <= 0 || !cue.text.trim()) continue
    const key = JSON.stringify([cue.text, cue.settings, cue.id])
    const last = lastByPayload.get(key)
    if (last && cue.start <= last.end + 0.0005) {
      last.end = Math.max(last.end, cue.end)
    } else {
      const entry = { ...cue }
      entries.push(entry)
      lastByPayload.set(key, entry)
    }
  }
  const rendered = entries.map(({ start, end, text, settings, id }) =>
    `${id ? id + '\n' : ''}${formatVttTime(start)} --> ${formatVttTime(end)}${settings ? ' ' + settings : ''}\n${text}`,
  )
  return new Blob(['WEBVTT\n\n', [...blocks, ...rendered].join('\n\n'), '\n'],
    { type: 'text/vtt;charset=utf-8' })
}
