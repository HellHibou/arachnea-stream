import type { ResolvedVideoMediaSource } from '@/services/players'

/** Timeout applied to storyboard VTT fetch requests. */
export const STORYBOARD_VTT_REQUEST_TIMEOUT_MS = 5_000
/** Display size of a storyboard preview frame. */
export const STORYBOARD_PREVIEW_FRAME_SIZE = 15
/** Display width, in pixels, of a storyboard preview frame. */
export const STORYBOARD_PREVIEW_WIDTH = 16 * STORYBOARD_PREVIEW_FRAME_SIZE
/** Display height, in pixels, of a storyboard preview frame. */
export const STORYBOARD_PREVIEW_HEIGHT = 9 * STORYBOARD_PREVIEW_FRAME_SIZE
/** Player width below which the sprite plugin applies its responsive factor. */
export const STORYBOARD_RESPONSIVE_WIDTH = 600
/** Gap kept between the storyboard tooltip and the seek bar. */
export const STORYBOARD_TOOLTIP_GAP = 12

/**
 * Cue geometry parsed from a storyboard WebVTT document.
 */
export type StoryboardVttCue = {
  imageUrl: string
  start: number
  end: number
  x: number
  y: number
  width: number
  height: number
}

/**
 * Resolved sprite thumbnail plugin configuration derived from storyboard metadata.
 */
export type ResolvedSpriteThumbnailOptions = {
  options: Record<string, unknown>
  cues: StoryboardVttCue[]
}

/**
 * Formats one sequential storyboard image index with its page offset and optional zero padding.
 *
 * @param index Zero-based image index selected for the current preview time.
 * @param firstPageIndex Index used by the first sprite image in the URL template.
 * @param indexPadding Zero-padding width applied to the formatted index.
 * @returns Image index ready to replace the `{index}` template placeholder.
 */
export function formatStoryboardIndex(
  index: number,
  firstPageIndex: number,
  indexPadding: number,
): string {
  const value = String(index + firstPageIndex)
  return indexPadding > 0 ? value.padStart(indexPadding, '0') : value
}

/**
 * Preloads the first sprite image and derives one cell's dimensions from its declared grid.
 *
 * @param url Sprite image URL, which may contain an `{index}` placeholder.
 * @param firstPageIndex First index used by a sequential sprite URL.
 * @param indexPadding Zero-padding width applied to the formatted index.
 * @param columns Number of sprite columns.
 * @param rows Number of sprite rows.
 * @returns Derived cell dimensions and a normalized sprite URL, or `null` when the grid is invalid.
 */
export function resolveSpriteThumbnailDimensions(
  url: string,
  firstPageIndex: number,
  indexPadding: number,
  columns: number,
  rows: number,
): Promise<{ url: string, width: number, height: number } | null> {
  const firstSpriteUrl = url.replace('{index}', formatStoryboardIndex(0, firstPageIndex, indexPadding))

  return new Promise((resolve) => {
    const image = new Image()
    image.onload = () => {
      const horizontalGap = findSpriteGridGap(image.naturalWidth, columns)
      const verticalGap = findSpriteGridGap(image.naturalHeight, rows)

      if (horizontalGap === null || verticalGap === null) {
        console.warn('[Storyboard] Sprite image dimensions are incompatible with its declared grid.', {
          url: firstSpriteUrl,
          naturalWidth: image.naturalWidth,
          naturalHeight: image.naturalHeight,
          columns,
          rows,
        })
        resolve(null)
        return
      }

      const width = (image.naturalWidth - horizontalGap * (columns - 1)) / columns
      const height = (image.naturalHeight - verticalGap * (rows - 1)) / rows

      if (horizontalGap === 0 && verticalGap === 0) {
        resolve({ url, width, height })
        return
      }

      if (url.includes('{index}')) {
        console.warn('[Storyboard] Cannot normalize grid separators in sequential sprite images.', {
          url: firstSpriteUrl,
        })
        resolve(null)
        return
      }

      try {
        const canvas = document.createElement('canvas')
        canvas.width = width * columns
        canvas.height = height * rows
        const context = canvas.getContext('2d')
        if (!context) {
          resolve(null)
          return
        }

        for (let row = 0; row < rows; row += 1) {
          for (let column = 0; column < columns; column += 1) {
            context.drawImage(
              image,
              column * (width + horizontalGap),
              row * (height + verticalGap),
              width,
              height,
              column * width,
              row * height,
              width,
              height,
            )
          }
        }

        resolve({ url: canvas.toDataURL('image/jpeg', 0.92), width, height })
      } catch (error) {
        console.warn('[Storyboard] Unable to normalize sprite image grid separators.', {
          url: firstSpriteUrl,
          error,
        })
        resolve(null)
      }
    }
    image.onerror = () => {
      console.warn('[Storyboard] Unable to preload sprite image for dimension inference.', {
        url: firstSpriteUrl,
      })
      resolve(null)
    }
    image.src = firstSpriteUrl
  })
}

/**
 * Finds the smallest uniform gap between cells in one sprite axis.
 *
 * @param size Natural image width or height.
 * @param cells Number of cells along that axis.
 * @returns Gap size in pixels, or `null` when no regular grid fits the image.
 */
export function findSpriteGridGap(size: number, cells: number): number | null {
  for (let gap = 0; gap <= 32; gap += 1) {
    const cellSize = size - gap * (cells - 1)
    if (cellSize > 0 && cellSize % cells === 0) {
      return gap
    }
  }

  return null
}

/**
 * Builds sprite thumbnail plugin options from storyboard metadata.
 *
 * Missing cell dimensions are inferred by preloading the first sprite image.
 *
 * @param source Resolved source selected for playback.
 * @returns Sprite thumbnail plugin configuration.
 */
export async function buildSpriteThumbnailOptions(
  source: ResolvedVideoMediaSource,
): Promise<Record<string, unknown>> {
  if (!source.storyboard) {
    return {}
  }

  const { firstPageIndex, indexPadding, interval, width, height, ...spriteThumbnails } = source.storyboard
  const resolvedSprite = width && height
    ? { url: source.storyboard.url, width, height }
    : await resolveSpriteThumbnailDimensions(
      source.storyboard.url,
      firstPageIndex,
      indexPadding,
      source.storyboard.columns,
      source.storyboard.rows,
    )

  if (!resolvedSprite) {
    return {}
  }

  const resolvedSpriteThumbnails = { ...spriteThumbnails, ...resolvedSprite }
  const options = interval === null
    ? resolvedSpriteThumbnails
    : { ...resolvedSpriteThumbnails, interval }

  return firstPageIndex > 0 || indexPadding > 0
    ? { ...options, idxTag: (index: number) => formatStoryboardIndex(index, firstPageIndex, indexPadding) }
    : options
}

/**
 * Converts a timestamp from a WebVTT cue into seconds.
 *
 * @param value WebVTT timestamp in `HH:MM:SS.mmm` form.
 * @returns Timestamp in seconds, or null when invalid.
 */
export function parseStoryboardVttTimestamp(value: string): number | null {
  const match = /^(?:(\d+):)?(\d{2}):(\d{2}(?:\.\d+)?)$/.exec(value.trim())
  if (!match) {
    return null
  }

  const hours = Number(match[1] ?? 0)
  const minutes = Number(match[2])
  const seconds = Number(match[3])
  const total = hours * 3_600 + minutes * 60 + seconds

  return Number.isFinite(total) && total >= 0 ? total : null
}

/**
 * Parses image cue geometry from a storyboard WebVTT document.
 *
 * @param documentText Raw WebVTT document.
 * @param vttUrl Absolute URL of the WebVTT document.
 * @returns Parsed cue metadata for one sprite image, or an empty list.
 */
export function parseStoryboardVttCues(documentText: string, vttUrl: string): StoryboardVttCue[] {
  const cuePattern = /(?:^|\n)\s*((?:\d+:)?\d{2}:\d{2}(?:\.\d+)?)\s+-->\s+((?:\d+:)?\d{2}:\d{2}(?:\.\d+)?)[^\n]*\n\s*([^\s#]+)#xywh=(\d+),(\d+),(\d+),(\d+)/g
  const cues: StoryboardVttCue[] = []

  for (const match of documentText.matchAll(cuePattern)) {
    const [, startText, endText, imageUrl, xText, yText, widthText, heightText] = match
    if (!startText || !endText || !imageUrl || !xText || !yText || !widthText || !heightText) {
      continue
    }

    const start = parseStoryboardVttTimestamp(startText)
    const end = parseStoryboardVttTimestamp(endText)
    const x = Number(xText)
    const y = Number(yText)
    const width = Number(widthText)
    const height = Number(heightText)

    if (
      start === null ||
      end === null ||
      end <= start ||
      !Number.isInteger(x) ||
      !Number.isInteger(y) ||
      !Number.isInteger(width) ||
      !Number.isInteger(height) ||
      width <= 0 ||
      height <= 0
    ) {
      continue
    }

    let resolvedImageUrl = imageUrl
    try {
      resolvedImageUrl = new URL(imageUrl, vttUrl).href
    } catch {
      continue
    }

    cues.push({
      imageUrl: resolvedImageUrl,
      start,
      end,
      x,
      y,
      width,
      height,
    })
  }

  return cues
}

/**
 * Derives sprite thumbnail plugin options from a storyboard WebVTT document.
 *
 * @param documentText Raw WebVTT document.
 * @param vttUrl Absolute URL of the WebVTT document.
 * @returns Plugin configuration, or null for unsupported cue layouts.
 */
export function parseStoryboardVttOptions(
  documentText: string,
  vttUrl: string,
): ResolvedSpriteThumbnailOptions | null {
  const cues = parseStoryboardVttCues(documentText, vttUrl)
  const firstCue = cues[0]

  if (!firstCue || cues.some((cue) =>
    cue.width !== firstCue.width ||
    cue.height !== firstCue.height,
  )) {
    if (!firstCue)
      return null

    cues.forEach(entry => {
      entry.width = firstCue.width
      entry.height = firstCue.height
    })
  }

  const urlArray = [...new Set(cues.map((cue) => cue.imageUrl))]
  const columns = Math.max(...cues.map((cue) => cue.x / cue.width + 1))
  const rows = Math.max(...cues.map((cue) => cue.y / cue.height + 1))
  const lastCue = cues[cues.length - 1]
  const interval = lastCue
    ? lastCue.end / cues.length
    : cues[1]
      ? cues[1].start - firstCue.start
      : firstCue.end - firstCue.start

  if (
    !Number.isInteger(columns) ||
    !Number.isInteger(rows) ||
    columns < 1 ||
    rows < 1 ||
    !Number.isFinite(interval) ||
    interval <= 0
  ) {
    return null
  }

  return {
    options: {
      urlArray,
      width: firstCue.width,
      height: firstCue.height,
      columns,
      rows,
      interval,
      downlink: 0,
    },
    cues,
  }
}

/**
 * Resolves VTT storyboard metadata to the sprite plugin configuration, retaining the declared
 * sprite storyboard as a fallback when the VTT cannot be fetched or parsed.
 *
 * @param source Resolved source selected for playback.
 * @returns Sprite thumbnail plugin configuration.
 */
export async function resolveSpriteThumbnailOptions(
  source: ResolvedVideoMediaSource,
): Promise<ResolvedSpriteThumbnailOptions> {
  if (!source.storyboardVttUrl) {
    return { options: await buildSpriteThumbnailOptions(source), cues: [] }
  }

  const controller = new AbortController()
  const timeout = window.setTimeout(() => controller.abort(), STORYBOARD_VTT_REQUEST_TIMEOUT_MS)

  try {
    const response = await fetch(source.storyboardVttUrl, { signal: controller.signal })
    if (!response.ok) {
      return { options: await buildSpriteThumbnailOptions(source), cues: [] }
    }

    return parseStoryboardVttOptions(await response.text(), source.storyboardVttUrl)
      ?? { options: await buildSpriteThumbnailOptions(source), cues: [] }
  } catch {
    return { options: await buildSpriteThumbnailOptions(source), cues: [] }
  } finally {
    window.clearTimeout(timeout)
  }
}
