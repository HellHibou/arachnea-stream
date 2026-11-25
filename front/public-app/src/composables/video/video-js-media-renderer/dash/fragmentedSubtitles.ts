import type { parseBuffer as parseIsoBuffer, IsoBox } from 'codem-isoboxer'
import { createWebVtt, fetchSubtitle, fetchSubtitleSegments, type SubtitleCue } from '../subtitleExport'
import { createStppReader } from './ttmlSubtitles'

export type DashSubtitleCodec = 'wvtt' | 'stpp'
export interface SubtitleResource { url: string; range?: string }
export interface DashSubtitleStream {
  initialization: SubtitleResource
  segments: SubtitleResource[]
  offset: number
}

interface TimedSample {
  bytes: ArrayBuffer
  start: number
  end: number
}

/**
 * Reads independently timed subtitle samples from the MP4 fragment's text track.
 * @param parseBuffer ISO BMFF parser loaded when the export begins.
 * @param buffer One downloaded media segment.
 * @param timescale Track timescale declared by the initialization segment.
 * @param defaults Default sample sizes and durations declared by the track.
 * @param rangeStart Offset in the original file for a byte-range response.
 */
function readSamples(parseBuffer: typeof parseIsoBuffer, buffer: ArrayBuffer, timescale: number, defaults: IsoBox | undefined, rangeStart: number): TimedSample[] {
  const file = parseBuffer(buffer)
  const result: TimedSample[] = []
  const movies = file.fetchAll('moof')
  if (!movies.length || !file.fetch('mdat') || file.fetch('senc')) throw new Error('Invalid or encrypted subtitle fragment')
  for (const movie of movies) {
    const tracks = movie.boxes?.filter((box) => box.type === 'traf') ?? []
    if (tracks.length !== 1) throw new Error('Unsupported multiplexed subtitle fragment')
    const track = tracks[0]!.boxes ?? []
    const header = track.find((box) => box.type === 'tfhd')
    let decodeTime = track.find((box) => box.type === 'tfdt')?.baseMediaDecodeTime
    const data = file.boxes.find((box) => box.type === 'mdat' && box._offset > movie._offset)
    if (!header || decodeTime === undefined || !data) throw new Error('Invalid subtitle fragment timing')
    const base = header.base_data_offset === undefined ? movie._offset : header.base_data_offset - rangeStart
    const mediaStart = data._offset + (new DataView(buffer).getUint32(data._offset) === 1 ? 16 : 8)
    let position = mediaStart
    for (const run of track.filter((box) => box.type === 'trun')) {
      if (run.data_offset !== undefined) position = base + run.data_offset
      for (const sample of run.samples ?? []) {
        const duration = sample.sample_duration ?? header.default_sample_duration ?? defaults?.default_sample_duration
        const size = sample.sample_size ?? header.default_sample_size ?? defaults?.default_sample_size
        if (!duration || size === undefined || position < mediaStart || position + size > data._offset + data.size ||
          position + size > buffer.byteLength) throw new Error('Invalid subtitle sample')
        const time = decodeTime + (sample.sample_composition_time_offset ?? 0)
        if (size) result.push({ bytes: buffer.slice(position, position + size), start: time / timescale, end: (time + duration) / timescale })
        position += size
        decodeTime += duration
      }
    }
  }
  return result
}

/**
 * Extracts WebVTT cue text and presentation settings from one MP4 sample.
 * @param parseBuffer ISO BMFF parser loaded when the export begins.
 * @param sample Timed sample from a WebVTT fragment.
 * @param offset Shift from the media track to the presentation timeline.
 */
function readWvtt(parseBuffer: typeof parseIsoBuffer, sample: TimedSample, offset: number): SubtitleCue[] {
  return parseBuffer(sample.bytes).boxes.flatMap((box) => {
    if (box.type === 'vtte') return []
    if (box.type !== 'vttc' || !box.boxes) throw new Error('Invalid WebVTT subtitle sample')
    const text = box.boxes.find((child) => child.type === 'payl')?.cue_text
    const settings = box.boxes.find((child) => child.type === 'sttg')?.settings
    return text ? [{ start: sample.start + offset, end: sample.end + offset, text, settings }] : []
  })
}

/**
 * Loads a DASH subtitle track independently of playback and exports one complete WebVTT file.
 * @param streams Ordered periods belonging to the track.
 * @param codec Subtitle codec declared by the manifest.
 * @param signal Export cancellation and timeout.
 */
export async function assembleDashSubtitles(streams: DashSubtitleStream[], codec: DashSubtitleCodec, signal: AbortSignal) {
  const { parseBuffer } = await import('codem-isoboxer')
  const cues: SubtitleCue[] = []
  const readStpp = codec === 'stpp' ? await createStppReader() : null
  for (const { initialization, segments, offset } of streams) {
    const buffers = await fetchSubtitleSegments([initialization, ...segments],
      async ({ url, range }) => (await fetchSubtitle(url, signal, range)).arrayBuffer(), signal)
    const init = parseBuffer(buffers[0]!)
    const timescale = init.fetch('mdhd')?.timescale
    if (!timescale || init.fetchAll('trak').length !== 1 || init.fetch('tenc')) {
      throw new Error('Unsupported subtitle initialization segment')
    }
    const defaults = init.fetch('trex')
    for (const [index, segment] of segments.entries()) {
      signal.throwIfAborted()
      const rangeStart = Number(/^bytes=(\d+)/.exec(segment.range ?? '')?.[1] ?? 0)
      for (const sample of readSamples(parseBuffer, buffers[index + 1]!, timescale, defaults, rangeStart)) {
        const entries = readStpp ? readStpp(sample, offset) : readWvtt(parseBuffer, sample, offset)
        for (const entry of entries) cues.push(entry)
      }
    }
  }
  return createWebVtt(cues)
}
