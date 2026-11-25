import { getDashMediaPlayer } from './source'
import {
  assembleDashSubtitles, type DashSubtitleStream, type SubtitleResource,
} from './fragmentedSubtitles'
import { MAX_SUBTITLE_SEGMENTS, type SubtitleDownload } from '../subtitleExport'
import type { VideoJsDashMediaPlayerHandle, VideoJsPlayer } from '../types'

type TextNode = { __text?: string } | string
interface TimelineEntry { t?: number; d: number; r?: number }
interface SegmentAddressing {
  initialization?: string
  media?: string
  startNumber?: number
  timescale?: number
  duration?: number
  presentationTimeOffset?: number
  SegmentTimeline?: { S_asArray?: TimelineEntry[] }
  Initialization?: { sourceURL?: string; range?: string }
  SegmentURL_asArray?: Array<{ media?: string; mediaRange?: string }>
}
interface ManifestNode {
  BaseURL_asArray?: TextNode[]
  SegmentBase?: unknown
  SegmentList?: SegmentAddressing
  SegmentTemplate?: SegmentAddressing
  ContentProtection_asArray?: unknown[]
  mimeType?: string
  codecs?: string
  lang?: string
  Label_asArray?: TextNode[]
  Role_asArray?: Array<{ value?: string }>
}
interface Representation extends ManifestNode {
  id?: string | number
  bandwidth?: number
}
interface Period extends ManifestNode {
  start?: number
  duration?: number
  AdaptationSet_asArray?: Array<ManifestNode & {
    Representation_asArray?: Representation[]
  }>
}
interface Manifest extends ManifestNode {
  url?: string
  baseUri?: string
  type?: string
  mediaPresentationDuration?: number
  Period_asArray?: Period[]
}

const subtitleFiles = new WeakMap<VideoJsDashMediaPlayerHandle, SubtitleDownload[]>()

/**
 * Reads the value of a DASH manifest element with or without attributes.
 * @param value Parsed text element.
 */
function readText(value: TextNode | undefined): string | undefined {
  return (typeof value === 'string' ? value : value?.__text)?.trim() || undefined
}

/**
 * Resolves a DASH resource against the inherited BaseURL.
 * @param value URL declared on the node.
 * @param base Parent base URL.
 */
function resolveBase(value: string | undefined, base: string): string {
  return new URL(value ?? base, base).href
}

/**
 * Expands DASH's resource template for a known subtitle representation.
 * @param template Template declaring the subtitle resource path.
 * @param representation Selected text representation.
 * @param time Segment timeline time.
 * @param number Segment number in the representation.
 */
function expandTemplate(template: string, representation: Representation, time = 0, number = 0): string {
  const tokens = { RepresentationID: representation.id, Bandwidth: representation.bandwidth, Time: time, Number: number }
  return template.replace(/\$\$|\$(RepresentationID|Bandwidth|Time|Number)(?:%0(\d+)d)?\$/g,
    (token, key: keyof typeof tokens, padding?: string) => {
      if (token === '$$') return '$'
      const value = tokens[key]
      if (value === undefined || Number(padding ?? 0) > 20) throw new Error('Invalid DASH subtitle template')
      return String(value).padStart(Number(padding ?? 0), '0')
    })
}

/**
 * Enumerates text fragments described by SegmentTemplate or SegmentList.
 * @param representation Selected text representation.
 * @param addressing Inherited template or segment list.
 * @param base Resolved representation BaseURL.
 * @param start Period start in seconds.
 * @param duration Period duration in seconds.
 */
function buildFragmentStream(
  representation: Representation, addressing: SegmentAddressing, base: string, start: number, duration: number | undefined,
): DashSubtitleStream {
  const timescale = addressing.timescale ?? 1
  const mediaOffset = addressing.presentationTimeOffset ?? 0
  if (!Number.isFinite(timescale) || timescale <= 0) throw new Error('Invalid DASH subtitle timescale')
  const offset = start - mediaOffset / timescale
  if (addressing.SegmentURL_asArray) {
    const init = addressing.Initialization
    if (!init) throw new Error('Missing subtitle initialization')
    const segments = addressing.SegmentURL_asArray.map(({ media, mediaRange }): SubtitleResource => ({
      url: resolveBase(media, base),
      range: mediaRange && 'bytes=' + mediaRange,
    }))
    return {
      initialization: { url: resolveBase(init.sourceURL, base), range: init.range && 'bytes=' + init.range },
      segments, offset,
    }
  }
  if (!addressing.initialization || !addressing.media) throw new Error('Incomplete DASH subtitle template')
  const initialization = { url: resolveBase(expandTemplate(addressing.initialization, representation), base) }
  const media = addressing.media
  const length = duration === undefined ? undefined : duration * timescale
  const timeline = addressing.SegmentTimeline?.S_asArray ?? (
    addressing.duration && length !== undefined
      ? [{ t: mediaOffset, d: addressing.duration, r: Math.ceil(length / addressing.duration) - 1 }]
      : []
  )
  let time = 0
  let number = addressing.startNumber ?? 1
  const segments: SubtitleResource[] = []
  for (const [index, entry] of timeline.entries()) {
    time = entry.t ?? time
    const repeat = entry.r ?? 0
    if (!Number.isFinite(time) || !Number.isFinite(entry.d) || entry.d <= 0 || !Number.isInteger(repeat)) {
      throw new Error('Invalid DASH subtitle timeline')
    }
    const nextBoundary = timeline[index + 1]?.t ?? (length === undefined ? NaN : mediaOffset + length)
    const count = repeat >= 0 ? repeat + 1 : Math.ceil((nextBoundary - time) / entry.d)
    if (!Number.isFinite(count) || count <= 0 || segments.length + count >= MAX_SUBTITLE_SEGMENTS) {
      throw new Error('Unsupported DASH subtitle timeline')
    }
    for (let index = 0; index < count; index++) {
      segments.push({ url: resolveBase(expandTemplate(media, representation, time, number++), base) })
      time += entry.d
    }
  }
  if (!segments.length) throw new Error('Empty DASH subtitle timeline')
  return { initialization, segments, offset }
}

/**
 * Records standalone subtitle files and exportable text tracks from the parsed DASH manifest.
 * @param mediaPlayer Current dash.js instance.
 * @param data Parsed manifest from dash.js.
 */
export function updateDashSubtitleDownloads(mediaPlayer: VideoJsDashMediaPlayerHandle, data: unknown): void {
  const manifest = data as Manifest | null
  const files: SubtitleDownload[] = []
  subtitleFiles.set(mediaPlayer, files)
  const manifestUrl = manifest?.baseUri || manifest?.url
  if (!manifest || !manifestUrl || manifest.ContentProtection_asArray?.length) return

  try {
    const manifestBase = resolveBase(readText(manifest.BaseURL_asArray?.[0]), resolveBase(manifestUrl, location.href))
    const streams = new Map<string, { download: SubtitleDownload; periods: DashSubtitleStream[] }>()
    let previousEnd: number | undefined = 0
    const periods = manifest.Period_asArray ?? []
    for (const [index, period] of periods.entries()) {
      const start: number | undefined = period.start ?? previousEnd
      const nextStart = periods[index + 1]?.start ?? (index === periods.length - 1 ? manifest.mediaPresentationDuration : undefined)
      const duration: number | undefined = period.duration ?? (nextStart !== undefined && start !== undefined ? nextStart - start : undefined)
      previousEnd = start !== undefined && duration !== undefined ? start + duration : undefined
      if (period.ContentProtection_asArray?.length) continue
      const periodBase = resolveBase(readText(period.BaseURL_asArray?.[0]), manifestBase)
      const trackOccurrences = new Map<string, number>()
      for (const adaptation of period.AdaptationSet_asArray ?? []) {
        if (adaptation.ContentProtection_asArray?.length) continue
        const adaptationBase = readText(adaptation.BaseURL_asArray?.[0])
        const parentBase = resolveBase(adaptationBase, periodBase)
        for (const representation of adaptation.Representation_asArray ?? []) {
          if (representation.ContentProtection_asArray?.length) continue
          const mime = representation.mimeType ?? adaptation.mimeType
          const codec = (representation.codecs ?? adaptation.codecs)?.split('.')[0]
          const extension = mime === 'text/vtt' ? 'vtt' : mime === 'application/ttml+xml' ? 'ttml' : null
          if (!extension && codec !== 'wvtt' && codec !== 'stpp') continue
          const repBase = readText(representation.BaseURL_asArray?.[0])
          const link = resolveBase(repBase, parentBase)
          const lang = representation.lang ?? adaptation.lang
          const label = readText(representation.Label_asArray?.[0]) ?? readText(adaptation.Label_asArray?.[0])
          const levels = [representation, adaptation, period, manifest]
          const addressing = levels.find((node) => node.SegmentList || node.SegmentTemplate || node.SegmentBase)
          if (extension && !addressing && (repBase || adaptationBase) && !new URL(link).pathname.endsWith('/')) {
            if (!files.some((file) => file.link === link)) files.push({ link, lang, label, extension })
            break
          } else if (manifest.type !== 'dynamic' && start !== undefined && (codec === 'wvtt' || codec === 'stpp')) {
            const template = addressing?.SegmentList ?? addressing?.SegmentTemplate
            if (!template) continue
            const roles = (representation.Role_asArray ?? adaptation.Role_asArray)?.map(({ value }) => value).sort()
            const identity = JSON.stringify([lang, label, roles, codec])
            const occurrence = trackOccurrences.get(identity) ?? 0
            trackOccurrences.set(identity, occurrence + 1)
            const key = `${identity}:${occurrence}`
            const stream = buildFragmentStream(representation, template, link, start, duration)
            const group = streams.get(key)
            if (group) group.periods.push(stream)
            else {
              const parts = [stream]
              streams.set(key, { periods: parts, download: {
                link: stream.initialization.url, lang, label, extension: 'vtt',
                assemble: (signal) => assembleDashSubtitles(parts, codec, signal),
              } })
            }
            break
          }
        }
      }
    }
    files.push(...[...streams.values()].map(({ download }) => download))
  } catch {
    // A malformed subtitle manifest must not prevent playback or advertise a partial export.
  }
}

/**
 * Reads subtitle downloads for the active DASH engine.
 * @param player Video.js player that owns the engine.
 */
export function getDashSubtitleDownloads(player: VideoJsPlayer): SubtitleDownload[] {
  const mediaPlayer = getDashMediaPlayer(player)
  return (mediaPlayer && subtitleFiles.get(mediaPlayer)) || []
}
