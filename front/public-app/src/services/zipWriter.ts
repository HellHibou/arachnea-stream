/** A single file to store in a ZIP archive. */
export interface ZipEntry {
  /** Forward-slash path of the file inside the archive. */
  name: string
  /** Raw file content. */
  data: Uint8Array
}

const CRC_TABLE = (() => {
  const table = new Uint32Array(256)
  for (let index = 0; index < 256; index += 1) {
    let value = index
    for (let bit = 0; bit < 8; bit += 1) {
      value = value & 1 ? 0xEDB88320 ^ (value >>> 1) : value >>> 1
    }
    table[index] = value >>> 0
  }
  return table
})()

/**
 * Computes the CRC-32 checksum used by the ZIP format.
 * @param data File content to checksum.
 * @returns Unsigned CRC-32 value.
 */
function crc32(data: Uint8Array): number {
  let crc = 0xFFFFFFFF
  for (const byte of data) {
    crc = (CRC_TABLE[(crc ^ byte) & 0xFF] ?? 0) ^ (crc >>> 8)
  }
  return (crc ^ 0xFFFFFFFF) >>> 0
}

/**
 * Builds the MS-DOS date and time fields expected in ZIP headers.
 * @returns Time and date values packed for a local or central directory header.
 */
function dosDateTime(): { time: number, date: number } {
  const now = new Date()
  const year = Math.max(1980, now.getFullYear())
  return {
    time: (now.getHours() << 11) | (now.getMinutes() << 5) | (now.getSeconds() >> 1),
    date: ((year - 1980) << 9) | ((now.getMonth() + 1) << 5) | now.getDate(),
  }
}

/**
 * Creates a ZIP archive from in-memory entries using the STORE method.
 *
 * Names are written as UTF-8 and sizes and checksums are known up front, so no
 * data descriptors or compression library are required. The returned Blob is a
 * complete archive ready for `downloadFile`.
 *
 * @param entries Files to add to the archive.
 * @returns ZIP archive as a Blob.
 */
export function createZip(entries: ZipEntry[]): Blob {
  const encoder = new TextEncoder()
  const { time, date } = dosDateTime()
  const prepared = entries.map((entry) => {
    const name = encoder.encode(entry.name.replace(/\\/g, '/'))
    return { name, data: entry.data, crc: crc32(entry.data), offset: 0 }
  })
  const localParts: Uint8Array[] = []
  const centralParts: Uint8Array[] = []
  let offset = 0

  for (const entry of prepared) {
    const local = new Uint8Array(30 + entry.name.length)
    const localView = new DataView(local.buffer)
    localView.setUint32(0, 0x04034B50, true)
    localView.setUint16(4, 20, true)
    localView.setUint16(6, 0x0800, true)
    localView.setUint16(8, 0, true)
    localView.setUint16(10, time, true)
    localView.setUint16(12, date, true)
    localView.setUint32(14, entry.crc, true)
    localView.setUint32(18, entry.data.length, true)
    localView.setUint32(22, entry.data.length, true)
    localView.setUint16(26, entry.name.length, true)
    localView.setUint16(28, 0, true)
    local.set(entry.name, 30)
    entry.offset = offset
    localParts.push(local, entry.data)
    offset += local.length + entry.data.length

    const central = new Uint8Array(46 + entry.name.length)
    const centralView = new DataView(central.buffer)
    centralView.setUint32(0, 0x02014B50, true)
    centralView.setUint16(4, 20, true)
    centralView.setUint16(6, 20, true)
    centralView.setUint16(8, 0x0800, true)
    centralView.setUint16(10, 0, true)
    centralView.setUint16(12, time, true)
    centralView.setUint16(14, date, true)
    centralView.setUint32(16, entry.crc, true)
    centralView.setUint32(20, entry.data.length, true)
    centralView.setUint32(24, entry.data.length, true)
    centralView.setUint16(28, entry.name.length, true)
    centralView.setUint32(42, entry.offset, true)
    central.set(entry.name, 46)
    centralParts.push(central)
  }

  const centralSize = centralParts.reduce((total, part) => total + part.length, 0)
  const end = new Uint8Array(22)
  const endView = new DataView(end.buffer)
  endView.setUint32(0, 0x06054B50, true)
  endView.setUint16(4, 0, true)
  endView.setUint16(6, 0, true)
  endView.setUint16(8, prepared.length, true)
  endView.setUint16(10, prepared.length, true)
  endView.setUint32(12, centralSize, true)
  endView.setUint32(16, offset, true)

  const output = new Uint8Array(offset + centralSize + end.length)
  let cursor = 0
  for (const part of [...localParts, ...centralParts, end]) {
    output.set(part, cursor)
    cursor += part.length
  }
  return new Blob([output], { type: 'application/zip' })
}
