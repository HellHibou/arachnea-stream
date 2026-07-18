import { getBanners, normalizeBannersResponse } from '@/services/rustify'
import type { HomeBanner, HomeCatalogData } from '@/types/home'

/**
 * Loads every deferred banner collection present in the catalog,
 * with concurrency limiting and idempotent merging by source+link.
 *
 * Banners are appended to the global list in arrival order.
 * Already loaded source+link pairs are skipped to prevent duplicates on refresh.
 *
 * @param catalog Catalog returned by the home or category endpoint.
 * @returns Catalog whose banners include every resolved deferred source.
 */
export async function loadDeferredBannerPages(
  catalog: HomeCatalogData,
): Promise<HomeCatalogData> {
  const collection = catalog.banners

  if (!collection.link || !collection.source) {
    return catalog
  }

  const taskKey = `${collection.source}|${collection.link}`
  const seenKeys = new Set<string>()
  const loadedEntries: HomeBanner[] = [...collection.entries]
  const tasks: Array<{ key: string; loader: () => Promise<HomeBanner[]> }> = []

  tasks.push({
    key: taskKey,
    loader: () => loadDeferredBannersFromSource(collection.source, collection.link!),
  })

  if (tasks.length === 0) {
    return catalog
  }

  const concurrency = 4
  let nextIndex = 0
  const arrivalResults: HomeBanner[] = []

  async function worker(): Promise<void> {
    while (nextIndex < tasks.length) {
      const task = tasks[nextIndex++]
      if (!task) {
        return
      }

      const { key, loader } = task
      try {
        const banners = await loader()
        if (!seenKeys.has(key)) {
          seenKeys.add(key)
          arrivalResults.push(...banners)
        }
      } catch {
        // A single failed source should not block the others
      }
    }
  }

  const workers = Array.from({ length: Math.min(concurrency, tasks.length) }, () => worker())
  await Promise.all(workers)

  loadedEntries.push(...arrivalResults)

  return {
    ...catalog,
    banners: {
      ...collection,
      entries: loadedEntries,
      link: undefined,
    },
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
