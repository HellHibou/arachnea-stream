import type { Ref } from 'vue'

import {
  deleteSection,
  saveSection,
  type HomeSectionRecord,
} from '@/services/homeSections'
import { BOOKMARKS_SECTION_PREFERENCE_KEY } from '@/services/entryBookmarks'
import type {
  MediaCardCollectionMode,
  ThumbnailImageFit,
  ThumbnailOrientation,
} from '@/types/media'

import type { SectionRecordDefaults } from './types'

/** Section cache operations exposed by the storage store. */
export interface SectionRecordsBridge {
  /** Toggles the pinned state of one section. */
  toggleSectionPinned: (sectionKey: string) => Promise<void>
  /** Moves one pinned section within the ordered pinned subset. */
  movePinnedSection: (
    sectionKey: string,
    direction: 'up' | 'down',
    visibleKeys?: string[],
  ) => Promise<void>
  /** Updates the collection mode for one section. */
  updateSectionCollectionMode: (
    sectionKey: string,
    mode: MediaCardCollectionMode,
    defaults?: Omit<SectionRecordDefaults, 'collectionMode'>,
  ) => Promise<void>
  /** Updates the thumbnail orientation for one section. */
  updateSectionThumbnailOrientation: (
    sectionKey: string,
    orientation: ThumbnailOrientation,
    defaults?: Omit<SectionRecordDefaults, 'thumbnailOrientation'>,
  ) => Promise<void>
  /** Updates the thumbnail image fit for one section. */
  updateSectionThumbnailImageFit: (
    sectionKey: string,
    imageFit: ThumbnailImageFit,
    defaults?: Omit<SectionRecordDefaults, 'thumbnailImageFit'>,
  ) => Promise<void>
}

/**
 * Creates the section cache operations bound to the store's reactive record list.
 *
 * @param sectionRecords Reactive cache of pinned section records.
 * @returns Section operations sharing the given cache.
 */
export function createSectionRecordsBridge(
  sectionRecords: Ref<HomeSectionRecord[]>,
): SectionRecordsBridge {
    /**
     * Toggles the pinned state of one section.
     *
     * Pinning creates a new record (or moves it to the end of the pinned order).
     * Unpinning deletes the record.
     *
     * @param sectionKey - Preference key of the section.
     */
    async function toggleSectionPinned(sectionKey: string): Promise<void> {
      // The local bookmarks section is always pinned and cannot be unpinned.
      if (sectionKey === BOOKMARKS_SECTION_PREFERENCE_KEY) {
        return
      }

      const existing = sectionRecords.value.find((r) => r.name === sectionKey)

      if (existing) {
        sectionRecords.value = sectionRecords.value.filter((r) => r.name !== sectionKey)
        await deleteSection(sectionKey)
        return
      }

      const nextOrder = sectionRecords.value.length
      const record: HomeSectionRecord = {
        name: sectionKey,
        order: nextOrder,
        collectionMode: 'single-row',
        thumbnailOrientation: 'portrait',
        thumbnailImageFit: 'cover',
      }
      sectionRecords.value = [...sectionRecords.value, record]
      await saveSection(record)
    }

    /**
     * Ensures a section record exists in the cache (and IndexedDB), creating it
     * with default pinned display configuration when missing.
     *
     * Used by always-pinned local sections (e.g. bookmarks) so configure and
     * move actions work before their lazy record creation completes.
     *
     * @param sectionKey - Preference key of the section.
     * @param defaults - Display options applied only when the record is missing.
     * @returns The existing or newly created record, or `null` on write failure.
     */
    async function ensureSectionRecord(
      sectionKey: string,
      defaults: SectionRecordDefaults = {},
    ): Promise<HomeSectionRecord | null> {
      const existing = sectionRecords.value.find((r) => r.name === sectionKey)

      if (existing) {
        return existing
      }

      const record: HomeSectionRecord = {
        name: sectionKey,
        order: sectionRecords.value.length,
        collectionMode: defaults.collectionMode ?? 'single-row',
        thumbnailOrientation: defaults.thumbnailOrientation ?? 'portrait',
        thumbnailImageFit: defaults.thumbnailImageFit ?? 'cover',
      }
      sectionRecords.value = [...sectionRecords.value, record]
      await saveSection(record)
      return record
    }

    /**
     * Moves one pinned section within the ordered pinned subset.
     *
     * Materializes a missing record with default display configuration before
     * swapping, so always-pinned local sections (e.g. bookmarks) are movable
     * even on first mount before their lazy record creation completes.
     *
     * @param sectionKey - Preference key of the section to move.
     * @param direction - Direction to move the section.
     * @param visibleKeys - Optional catalog-scoped order; other catalogs are never swapped.
     */
    async function movePinnedSection(sectionKey: string, direction: 'up' | 'down', visibleKeys?: string[]): Promise<void> {
      await ensureSectionRecord(sectionKey)

      const ordered = visibleKeys
        ? visibleKeys.flatMap((key) => sectionRecords.value.filter((record) => record.name === key))
        : [...sectionRecords.value].sort((a, b) => a.order - b.order)
      const currentIndex = ordered.findIndex((r) => r.name === sectionKey)

      if (currentIndex < 0) {
        return
      }

      const targetIndex = direction === 'up' ? currentIndex - 1 : currentIndex + 1
      if (targetIndex < 0 || targetIndex >= ordered.length) {
        return
      }

      const target = ordered[targetIndex]
      const current = ordered[currentIndex]
      if (!target || !current) {
        return
      }
      const currentOrder = current.order
      const targetOrder = target.order

      // Swap the order values
      const nextRecords = sectionRecords.value.map((r) => {
        if (r.name === sectionKey) return { ...r, order: targetOrder }
        if (r.name === target.name) return { ...r, order: currentOrder }
        return r
      })
      sectionRecords.value = nextRecords

      await Promise.all([
        saveSection({ ...current, order: targetOrder } as HomeSectionRecord),
        saveSection({ ...target, order: currentOrder } as HomeSectionRecord),
      ])
    }

    /**
     * Updates the collection mode for one section.
     *
     * If the section is not pinned yet, this is a no-op.
     *
     * @param sectionKey - Preference key of the section.
     * @param mode - New collection mode.
     * @param defaults - Display options applied only when the record is missing.
     */
    async function updateSectionCollectionMode(
      sectionKey: string,
      mode: MediaCardCollectionMode,
      defaults: Omit<SectionRecordDefaults, 'collectionMode'> = {},
    ): Promise<void> {
      const record = await ensureSectionRecord(sectionKey, { ...defaults, collectionMode: mode })
      if (!record) {
        return
      }

      const nextRecord = { ...record, collectionMode: mode }
      sectionRecords.value = sectionRecords.value.map((r) =>
        r.name === sectionKey ? nextRecord : r,
      )
      await saveSection(nextRecord)
    }

    /**
     * Updates the thumbnail orientation for one section.
     *
     * If the section is not pinned yet, this is a no-op.
     *
     * @param sectionKey - Preference key of the section.
     * @param orientation - New thumbnail orientation.
     * @param defaults - Display options applied only when the record is missing.
     */
    async function updateSectionThumbnailOrientation(
      sectionKey: string,
      orientation: ThumbnailOrientation,
      defaults: Omit<SectionRecordDefaults, 'thumbnailOrientation'> = {},
    ): Promise<void> {
      const record = await ensureSectionRecord(sectionKey, { ...defaults, thumbnailOrientation: orientation })
      if (!record) {
        return
      }

      const nextRecord = { ...record, thumbnailOrientation: orientation }
      sectionRecords.value = sectionRecords.value.map((r) =>
        r.name === sectionKey ? nextRecord : r,
      )
      await saveSection(nextRecord)
    }

    /**
     * Updates the thumbnail image fit for one section.
     *
     * If the section is not pinned yet, this is a no-op.
     *
     * @param sectionKey - Preference key of the section.
     * @param imageFit - New thumbnail image fit.
     * @param defaults - Display options applied only when the record is missing.
     */
    async function updateSectionThumbnailImageFit(
      sectionKey: string,
      imageFit: ThumbnailImageFit,
      defaults: Omit<SectionRecordDefaults, 'thumbnailImageFit'> = {},
    ): Promise<void> {
      const record = await ensureSectionRecord(sectionKey, { ...defaults, thumbnailImageFit: imageFit })
      if (!record) {
        return
      }

      const nextRecord = { ...record, thumbnailImageFit: imageFit }
      sectionRecords.value = sectionRecords.value.map((r) =>
        r.name === sectionKey ? nextRecord : r,
      )
      await saveSection(nextRecord)
    }

  return {
    toggleSectionPinned,
    movePinnedSection,
    updateSectionCollectionMode,
    updateSectionThumbnailOrientation,
    updateSectionThumbnailImageFit,
  }
}
