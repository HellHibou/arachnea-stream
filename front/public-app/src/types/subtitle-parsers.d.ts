declare module 'm3u8-parser' {
  export class Parser {
    manifest: unknown
    push(text: string): void
    end(): void
  }
}

declare module 'codem-isoboxer' {
  export interface IsoBox {
    type: string
    _offset: number
    size: number
    boxes?: IsoBox[]
    timescale?: number
    baseMediaDecodeTime?: number
    base_data_offset?: number
    data_offset?: number
    default_sample_duration?: number
    default_sample_size?: number
    cue_text?: string
    settings?: string
    samples?: Array<{
      sample_duration?: number
      sample_size?: number
      sample_composition_time_offset?: number
    }>
  }
  export function parseBuffer(buffer: ArrayBuffer): {
    boxes: IsoBox[]
    fetch(type: string): IsoBox | undefined
    fetchAll(type: string): IsoBox[]
  }
}

declare module 'imsc/dist/imsc.all.min.js' {
  interface Document {
    getMediaTimeEvents(): number[]
  }
  interface Element {
    kind: string
    text?: string
    contents?: Element[]
    styleAttrs?: Record<string, unknown>
  }
  export function fromXML(xml: string): Document
  export function generateISD(document: Document, time: number): { contents: Element[] }
}
