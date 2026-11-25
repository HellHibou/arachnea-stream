import { resolveAppPath } from '@/services/baseUrl'

/**
 * Cleans a filename component for filesystem-safe use: strips diacritics,
 * replaces reserved characters and removes trailing dots or spaces.
 * @param value Original metadata.
 * @param fallback Value used when the cleaned metadata is empty.
 */
export function cleanFileName(value: string | null | undefined, fallback: string): string {
  return value?.normalize('NFD')
    .replace(/\p{Diacritic}/gu, '')
    .replace(/[<>:"/\\|?*]/g, '_')
    .replace(/[\u0000-\u001f]/g, '')
    .trim().replace(/[. ]+$/, '') || fallback
}

/**
 * Starts a browser download from a same-origin or Blob URL.
 * @param link File URL or Blob to download.
 * @param name Suggested local filename.
 * @param preserveName Whether the name should override any upstream attachment filename.
 */
export async function downloadFile(link: string | Blob, name: string, preserveName = false): Promise<void> {
  let objectUrl: string | undefined
  if (typeof link === 'string') {
    const url = new URL(link, document.baseURI)
    const sameOrigin = url.protocol === location.protocol && url.host === location.host
    const webUrl = url.protocol === 'https:' || url.protocol === 'http:'
    if (!sameOrigin && !webUrl && url.protocol !== 'blob:') throw new Error('Unsupported download URL')
    // Keep the signed RSS redirect on the media proxy, which preserves the attachment header.
    const odyseeAttachment = url.origin === 'https://odysee.com' && /^\/\$\/rss\/media\/[^/]+\/[a-f0-9]{40}\/stream\.mp4$/i.test(url.pathname)
    link = odyseeAttachment ? resolveAppPath(`api/proxy/${url.href}`) : url.href
    if (url.protocol !== 'blob:' && (preserveName || (webUrl && !sameOrigin && !odyseeAttachment))) {
      const response = await fetch(link)
      if (!response.ok) throw new Error(`Download failed (HTTP ${response.status})`)
      link = await response.blob()
    }
  }

  const anchor = document.createElement('a')
  anchor.href = typeof link === 'string' ? link : (objectUrl = URL.createObjectURL(link))
  anchor.download = name
  anchor.target = '_self'
  document.body.append(anchor)
  anchor.click()
  anchor.remove()
  if (objectUrl) {
    const url = objectUrl
    window.setTimeout(() => URL.revokeObjectURL(url), 60_000)
  }
}

/** Indicates that the browser returned a blank frame from protected playback. */
export class ProtectedFrameError extends Error {}

/**
 * Encodes the current decoded frame at its native resolution.
 * @param video Video element with a decoded frame.
 * @param protectedPlayback Whether the active stream negotiated content protection.
 */
function encodeFrame(video: HTMLVideoElement, protectedPlayback = false): Promise<Blob> {
  const canvas = document.createElement('canvas')
  canvas.width = video.videoWidth
  canvas.height = video.videoHeight
  const context = canvas.getContext('2d')
  if (!context) throw new Error('Canvas rendering is unavailable')

  context.drawImage(video, 0, 0)
  if (protectedPlayback) {
    const { data } = context.getImageData(0, 0, canvas.width, canvas.height)
    let hasImage = false
    for (let i = 0; i < data.length; i += 4) {
      if (data[i + 3] && (data[i] || data[i + 1] || data[i + 2])) {
        hasImage = true
        break
      }
    }
    if (!hasImage) throw new ProtectedFrameError('The browser returned a blank protected frame')
  }
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) resolve(blob)
      else reject(new Error('Video frame could not be encoded'))
    }, 'image/png')
  })
}

/**
 * Captures the displayed frame, retrying native cross-origin playback via a CORS-enabled element.
 * @param video Active video element.
 * @param protectedPlayback Whether the current source advertises content protection.
 */
export async function captureVideoFrame(video: HTMLVideoElement, protectedPlayback = false): Promise<Blob> {
  const { currentSrc, currentTime } = video
  const protectedFrame = protectedPlayback || Boolean(video.mediaKeys)
  try {
    return await encodeFrame(video, protectedFrame)
  } catch (error) {
    if (protectedFrame && error instanceof DOMException && error.name === 'SecurityError') {
      throw new ProtectedFrameError('The browser did not expose the protected frame')
    }
    if (!(error instanceof DOMException) || error.name !== 'SecurityError' ||
      !/^https?:\/\//i.test(currentSrc) || video.crossOrigin !== null) throw error
  }

  const capture = document.createElement('video')
  capture.crossOrigin = 'anonymous'
  capture.preload = 'auto'
  const listeners = new AbortController()
  let timeout = 0
  try {
    await new Promise<void>((resolve, reject) => {
      const options = { signal: listeners.signal }
      capture.addEventListener('error', () => reject(new Error('Video frame is unavailable')), options)
      capture.addEventListener('loadedmetadata', () => {
        if (currentTime > 0) capture.currentTime = currentTime
      }, options)
      capture.addEventListener('loadeddata', () => {
        if (currentTime === 0) resolve()
      }, options)
      capture.addEventListener('seeked', () => resolve(), options)
      timeout = window.setTimeout(() => reject(new Error('Video frame request timed out')), 10_000)
      capture.src = currentSrc
    })
    return await encodeFrame(capture)
  } finally {
    window.clearTimeout(timeout)
    listeners.abort()
    capture.removeAttribute('src')
    capture.load()
  }
}
