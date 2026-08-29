import { shallowRef } from 'vue'

/**
 * Image URLs that failed to load during the current session.
 *
 * The set is shared across media surfaces (<img> fallbacks in the player) and the
 * page-level poster resolvers so a broken URL behaves exactly like a `null`
 * candidate: it is skipped and the next available fallback image is used.
 */
const failedImageUrls = new Set<string>()

/** Reactive snapshot forwarded to computed resolvers so they re-evaluate on change. */
const failedImageUrlsSnapshot = shallowRef<ReadonlySet<string>>(new Set())

/**
 * Records that the given image URL could not be loaded.
 *
 * @param url Image URL that raised a loading error.
 */
export function markImageUrlFailed(url: string | null | undefined): void {
  if (!url || failedImageUrls.has(url)) {
    return
  }

  failedImageUrls.add(url)
  // Replace the snapshot so Vue computed properties re-running resolvers
  // observe the change and pick the next fallback image.
  failedImageUrlsSnapshot.value = new Set(failedImageUrls)
}

/**
 * Returns the first candidate image URL that has not failed to load.
 *
 * Failed URLs are skipped exactly like `null` candidates, which lets a poster
 * fallback chain advance to the next available image.
 *
 * @param candidates Ordered image URL candidates, from most to least preferred.
 */
export function resolveImageUrl(
  candidates: readonly (string | null | undefined)[],
): string | null {
  // Read the reactive snapshot so callers can use this inside a computed and
  // re-resolve whenever a new URL is marked as failed.
  const failed = failedImageUrlsSnapshot.value

  for (const candidate of candidates) {
    if (!candidate || failed.has(candidate)) {
      continue
    }
    return candidate
  }
  return null
}

