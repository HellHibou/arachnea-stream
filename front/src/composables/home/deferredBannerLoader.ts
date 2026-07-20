import { getBanners, normalizeBannersResponse } from '@/services/rustify'
import type { HomeBanner, HomeCatalogData } from '@/types/home'

/**
 * Loads deferred banner collections with concurrency limiting and idempotent merging by source+link.
 *
 * The callback is invoked after each successful source response so the UI can append those banners
 * without waiting for the remaining requests.
 *
 * @param catalog Catalog returned by the home or category endpoint.
 * @param onCatalogUpdate Callback receiving the catalog after each successful deferred response.
 * @returns Catalog whose deferred banner links have been consumed.
 */
export async function loadDeferredBannerPages(
  catalog: HomeCatalogData,
  onCatalogUpdate?: (catalog: HomeCatalogData) => void,
): Promise<HomeCatalogData> {
  const seenKeys = new Set<string>()
  const loadedEntries: HomeBanner[] = [...catalog.banners.entries]
  const tasks: Array<{ key: string; loader: () => Promise<HomeBanner[]> }> = []

  catalog.deferredBanners.forEach((collection) => {
    if (!collection.source || !collection.link) {
      return
    }

    const key = `${collection.source}|${collection.link}`
    tasks.push({
      key,
      loader: () => loadDeferredBannersFromSource(collection.source, collection.link!),
    })
  })

  if (tasks.length === 0) {
    return catalog
  }

  const concurrency = 4
  let nextIndex = 0

  async function worker(): Promise<void> {
    while (nextIndex < tasks.length) {
      const task = tasks[nextIndex++]
      if (!task || seenKeys.has(task.key)) {
        continue
      }

      seenKeys.add(task.key)

      try {
        const banners = await task.loader()
        loadedEntries.push(...banners)
        onCatalogUpdate?.({
          ...catalog,
          banners: { ...catalog.banners, entries: [...loadedEntries] },
        })
      } catch {
        // call_api already queued the technical failure; this worker must keep other sources running.
      }
    }
  }

  const workers = Array.from({ length: Math.min(concurrency, tasks.length) }, () => worker())
  await Promise.all(workers)

  return {
    ...catalog,
    banners: { ...catalog.banners, entries: loadedEntries },
    deferredBanners: catalog.deferredBanners.map((collection) => ({ ...collection, link: undefined })),
  }
}

/**
 * Loads one deferred banners collection and returns its normalized entries.
 *
 * @param source Backend source name.
 * @param link Banner link returned by the initial response.
 * @returns Normalized banners for this source.
 */
async function loadDeferredBannersFromSource(
  source: string,
  link: string,
): Promise<HomeBanner[]> {
  const response = await getBanners(source, link)
  return normalizeBannersResponse(response, source)
}
