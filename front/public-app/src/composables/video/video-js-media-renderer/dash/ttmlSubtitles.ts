import type { generateISD } from 'imsc/dist/imsc.all.min.js'
import type { SubtitleCue } from '../subtitleExport'

type Content = ReturnType<typeof generateISD>['contents'][number]
const decoder = new TextDecoder('utf-8', { fatal: true })

/**
 * Reads visible text from the TTML composition at one instant.
 * @param item Rendered subtitle element.
 */
function visibleText(item: Content): string {
  if (item.kind === 'image' || Object.keys(item.styleAttrs ?? {}).some((key) => key.endsWith(' backgroundImage'))) {
    throw new Error('Bitmap TTML subtitles cannot be exported as WebVTT text')
  }
  if (item.kind === 'br') return '\n'
  if (typeof item.text === 'string') return item.text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
  const text = (item.contents ?? []).map(visibleText).join('')
  return item.kind === 'p' && text ? text + '\n' : text
}

/**
 * Loads the TTML parser on demand and creates a reader for one track export.
 */
export async function createStppReader() {
  const { fromXML, generateISD } = await import('imsc/dist/imsc.all.min.js')

  /**
   * Converts a text TTML sample to cues clipped to the sample's presentation interval.
   * @param sample TTML document and validity interval.
   * @param offset Shift from the media track to the presentation timeline.
   */
  return (sample: { bytes: ArrayBuffer; start: number; end: number }, offset: number): SubtitleCue[] => {
    const xml = decoder.decode(sample.bytes)
    const document = fromXML(xml)
    const times = [sample.start, ...document.getMediaTimeEvents().filter((time) => time > sample.start && time < sample.end), sample.end]
    const cues: SubtitleCue[] = []
    for (let index = 0; index < times.length - 1; index++) {
      const start = times[index]!
      const end = times[index + 1]!
      const text = generateISD(document, start).contents.map(visibleText).join('').trim()
      if (text) cues.push({ start: start + offset, end: end + offset, text })
    }
    return cues
  }
}
