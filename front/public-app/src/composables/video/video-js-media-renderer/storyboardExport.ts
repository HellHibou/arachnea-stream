import type { ResolvedVideoMediaSource } from '@/services/players'
import { createZip, type ZipEntry } from '@/services/zipWriter'
import {
  formatStoryboardIndex,
  parseStoryboardVttCues,
  resolveSpriteThumbnailDimensions,
  STORYBOARD_VTT_REQUEST_TIMEOUT_MS,
  type StoryboardVttCue,
} from './storyboard'
import { createWebVtt } from './subtitleExport'

/** Maximum number of cues included in one storyboard archive. */
const MAX_STORYBOARD_FRAMES = 1200

/**
 * Extracts a readable reason from a thrown value for diagnostic reporting.
 * @param error Thrown value.
 * @returns Technical reason to show in error notifications.
 */
function describeError(error: unknown): string {
  return error instanceof Error ? error.message || error.name : String(error)
}

/**
 * Derives a cancellable signal that also expires after a fixed delay.
 * @param signal Parent signal propagated by the download menu.
 * @param timeoutMs Delay before the derived signal is aborted.
 * @returns Derived signal and a function releasing the timeout and listener.
 */
function withTimeout(signal: AbortSignal, timeoutMs: number): { signal: AbortSignal, clear: () => void } {
  const controller = new AbortController()
  const onAbort = () => controller.abort(signal.reason)
  if (signal.aborted) onAbort()
  else signal.addEventListener('abort', onAbort, { once: true })
  const timer = window.setTimeout(() => {
    controller.abort(new DOMException('Storyboard VTT request timed out', 'TimeoutError'))
  }, timeoutMs)
  return {
    signal: controller.signal,
    clear: () => {
      window.clearTimeout(timer)
      signal.removeEventListener('abort', onAbort)
    },
  }
}

/**
 * Checks that a sprite URL can be fetched by the browser.
 * @param url Absolute or relative image URL.
 */
function isFetchableImageUrl(url: string): boolean {
  try {
    const parsed = new URL(url, document.baseURI)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
      || parsed.protocol === 'blob:' || parsed.protocol === 'data:'
  } catch {
    return false
  }
}

/**
 * Names one archived sprite after its source extension.
 * @param url Absolute sprite URL.
 * @param index Zero-based position among the archive's unique sprites.
 * @returns Safe filename inside the archive.
 */
function spriteFileName(url: string, index: number): string {
  let extension = 'jpg'
  try {
    const match = /\.([a-z0-9]+)$/i.exec(new URL(url, document.baseURI).pathname)
    if (match?.[1]) extension = match[1].toLowerCase()
  } catch {
    // Keep the default extension when the URL cannot be parsed.
  }
  return `thumbnail-${String(index + 1).padStart(4, '0')}.${extension}`
}

/**
 * Builds thumbnail geometry from the declared sprite template when no usable VTT exists.
 * @param source Resolved source carrying the sprite metadata.
 * @param duration Video duration in seconds.
 * @param signal Cancellation signal.
 * @returns Ordered storyboard cues.
 */
async function buildTemplateFrames(
  source: ResolvedVideoMediaSource,
  duration: number,
  signal: AbortSignal,
): Promise<StoryboardVttCue[]> {
  const storyboard = source.storyboard
  if (!storyboard) throw new Error('The source declares no sprite storyboard')

  const { columns, rows } = storyboard
  if (!Number.isInteger(columns) || columns <= 0 || !Number.isInteger(rows) || rows <= 0) {
    throw new Error(`Invalid storyboard grid (${columns}x${rows})`)
  }

  let url = storyboard.url
  let width = storyboard.width
  let height = storyboard.height
  if (!width || !height) {
    const dimensions = await resolveSpriteThumbnailDimensions(
      url, storyboard.firstPageIndex, storyboard.indexPadding, columns, rows,
    )
    signal.throwIfAborted()
    if (!dimensions) {
      throw new Error(`Unable to infer cell dimensions from the first sprite (${url})`)
    }
    url = dimensions.url
    width = dimensions.width
    height = dimensions.height
  }
  if (width === undefined || height === undefined) {
    throw new Error(`Unable to determine storyboard cell dimensions (${url})`)
  }

  const cellWidth = Math.max(1, Math.round(width))
  const cellHeight = Math.max(1, Math.round(height))
  const interval = storyboard.interval ?? (duration > 0 ? duration / (columns * rows) : null)
  if (!interval || !Number.isFinite(interval) || interval <= 0) {
    throw new Error(`Unable to determine a storyboard interval (duration: ${duration}s, grid: ${columns}x${rows})`)
  }
  const cellsPerSprite = columns * rows
  const sequential = storyboard.url.includes('{index}')
  const count = Math.min(
    Math.ceil(duration / interval),
    MAX_STORYBOARD_FRAMES,
    sequential ? Number.POSITIVE_INFINITY : cellsPerSprite,
  )
  if (count < 1) throw new Error(`No thumbnail can be generated from the video duration (${duration}s)`)

  const frames: StoryboardVttCue[] = []
  for (let index = 0; index < count; index += 1) {
    const page = Math.floor(index / cellsPerSprite)
    const cell = index % cellsPerSprite
    frames.push({
      imageUrl: url.replace('{index}', formatStoryboardIndex(page, storyboard.firstPageIndex, storyboard.indexPadding)),
      start: index * interval,
      end: Math.min((index + 1) * interval, duration),
      x: (cell % columns) * cellWidth,
      y: Math.floor(cell / columns) * cellHeight,
      width: cellWidth,
      height: cellHeight,
    })
  }
  return frames
}

/**
 * Resolves the thumbnail list from the storyboard VTT, falling back to the sprite template.
 * @param source Resolved source carrying storyboard metadata.
 * @param duration Video duration in seconds.
 * @param signal Cancellation signal.
 * @returns Ordered storyboard cues.
 */
async function resolveStoryboardFrames(
  source: ResolvedVideoMediaSource,
  duration: number,
  signal: AbortSignal,
): Promise<StoryboardVttCue[]> {
  let vttFailure: string | null = null
  if (source.storyboardVttUrl) {
    const timed = withTimeout(signal, STORYBOARD_VTT_REQUEST_TIMEOUT_MS)
    try {
      const response = await fetch(source.storyboardVttUrl, { signal: timed.signal })
      if (!response.ok) {
        vttFailure = `VTT request returned HTTP ${response.status}`
      } else {
        const cues = parseStoryboardVttCues(await response.text(), source.storyboardVttUrl)
          .sort((left, right) => left.start - right.start)
        if (cues.length > 0) {
          return cues.slice(0, MAX_STORYBOARD_FRAMES)
        }
        vttFailure = 'VTT contains no usable #xywh image cues'
      }
    } catch (error) {
      signal.throwIfAborted()
      vttFailure = `VTT request failed: ${describeError(error)}`
    } finally {
      timed.clear()
    }
  }

  try {
    return await buildTemplateFrames(source, duration, signal)
  } catch (error) {
    signal.throwIfAborted()
    if (vttFailure) {
      throw new Error(
        `Storyboard VTT unavailable (${vttFailure}); sprite template unavailable (${describeError(error)})`,
      )
    }
    throw error
  }
}

/**
 * Downloads one sprite image without altering its bytes.
 * @param url Sprite URL.
 * @param signal Cancellation signal.
 * @returns Original image bytes.
 */
async function fetchSprite(url: string, signal: AbortSignal): Promise<Uint8Array> {
  let response: Response
  try {
    response = await fetch(url, { signal })
  } catch (error) {
    signal.throwIfAborted()
    throw new Error(`Unable to fetch storyboard sprite (${url}): ${describeError(error)}`)
  }
  if (!response.ok) throw new Error(`Storyboard sprite request failed (HTTP ${response.status}): ${url}`)
  try {
    return new Uint8Array(await response.arrayBuffer())
  } catch (error) {
    signal.throwIfAborted()
    throw new Error(`Unable to read storyboard sprite (${url}): ${describeError(error)}`)
  }
}

/**
 * Builds a ZIP archive containing the original storyboard sprite images and a
 * WebVTT document whose `#xywh` cues reference those local images.
 *
 * @param source Resolved source carrying storyboard metadata.
 * @param duration Video duration in seconds, used by the sprite template fallback.
 * @param signal Cancellation signal provided by the download menu.
 * @returns Complete ZIP archive as a Blob.
 */
export async function createStoryboardArchive(
  source: ResolvedVideoMediaSource,
  duration: number,
  signal: AbortSignal,
): Promise<Blob> {
  const frames = await resolveStoryboardFrames(source, duration, signal)
  signal.throwIfAborted()
  if (frames.length === 0) throw new Error('No storyboard cues available')

  const sprites = new Map<string, string>()
  for (const frame of frames) {
    if (!sprites.has(frame.imageUrl)) {
      sprites.set(frame.imageUrl, spriteFileName(frame.imageUrl, sprites.size))
    }
    if (frame.x < 0 || frame.y < 0 || frame.width <= 0 || frame.height <= 0) {
      throw new Error(`Invalid storyboard cue geometry at ${frame.start}s: ${frame.x}, ${frame.y}, ${frame.width}x${frame.height}`)
    }
  }

  const vtt = createWebVtt(frames.map((frame) => {
    const name = sprites.get(frame.imageUrl)
    if (!name) throw new Error(`Missing sprite filename for ${frame.imageUrl}`)
    return {
      start: frame.start,
      end: frame.end,
      text: `${name}#xywh=${frame.x},${frame.y},${frame.width},${frame.height}`,
    }
  }))
  const entries: ZipEntry[] = [{
    name: 'storyboard.vtt',
    data: new Uint8Array(await vtt.arrayBuffer()),
  }]

  for (const [url, name] of sprites) {
    signal.throwIfAborted()
    if (!isFetchableImageUrl(url)) throw new Error(`Unsupported storyboard image URL: ${url}`)
    entries.push({ name, data: await fetchSprite(url, signal) })
  }

  signal.throwIfAborted()
  return createZip(entries)
}
