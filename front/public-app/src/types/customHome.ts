import type { HomeCatalogData } from '@/types/home'
import type { MediaCardCollectionMode, ThumbnailOrientation, ThumbnailImageFit } from '@/types/media'

/** Optional block defaults saved with the draft; older definitions remain valid. */
export interface CustomHomeDisplayOptions {
  collectionMode?: MediaCardCollectionMode
  thumbnailOrientation?: ThumbnailOrientation
  thumbnailImageFit?: ThumbnailImageFit
}

/** Versioned, local definition; fetched content and pagination are never persisted. */
export interface CustomHomeDefinition {
  schemaVersion: 1
  id: string
  name: string
  navigationOrder: number
  bannerSources: string[]
  categoryShortcuts: CategoryReference[]
  blocks: CustomHomeBlock[]
  sectionMergeMode: 'none' | 'title'
  showBookmarks: boolean
  createdAt: number
  updatedAt: number
}

/** Source-owned category descriptor snapshot, to be rediscovered before execution. */
export interface CategoryReference {
  sourceId: string
  categoryKey: string
  descriptor: Record<string, string>
  labelSnapshot: string
}

/** Source-owned section identity, independent of display and preference keys. */
export interface SectionReference {
  sourceId: string
  sectionKey: string
  subsectionKey?: string
  labelSnapshot: string | null
}

/** Repeated references are allowed; every block owns a distinct opaque id. */
export type CustomHomeBlock = { displayOptions?: CustomHomeDisplayOptions } & (
  | { id: string; kind: 'home-section'; reference: SectionReference; titleOverride: string | null }
  | { id: string; kind: 'category-content'; reference: CategoryReference; titleOverride: string | null }
)

/** Identity quality lets the editor identify fragile or ambiguous selections. */
export interface HomeContentIdentity {
  sourceId: string
  key: string
  basis: 'provider' | 'descriptor' | 'label'
  ambiguous: boolean
}

/** Lightweight discovery retaining all source rows without semantic merging or hydration. */
export interface HomeSourceDiscovery {
  sourceId: string
  /** False when no source row survived aggregation; distinct from an available empty home. */
  available: boolean
  catalog: HomeCatalogData
}

/** Invalid records remain stored and are reported separately from usable definitions. */
export interface CustomHomeReadIssue {
  id: string | null
  message: string
}

export interface CustomHomeListResult {
  homes: CustomHomeDefinition[]
  issues: CustomHomeReadIssue[]
}

/** UI-only add intent; the destination and references are explicitly chosen before saving. */
export type CustomHomeAddIntent =
  | { kind: 'categories'; categories: import('@/types/home').HomeCategory[] }
  | { kind: 'banners'; sourceId: string }
  | { kind: 'category'; category: import('@/types/home').HomeCategory }
  | { kind: 'section'; section: import('@/types/home').HomeSection }

/** Picker selection remains detached from storage until the editor is saved. */
export type CustomHomeSelection =
  | { kind: 'banners'; sourceId: string }
  | { kind: 'shortcut'; reference: CategoryReference }
  | { kind: 'block'; block: CustomHomeBlock }