import type { CustomHomeDefinition } from '@/types/customHome'

/** @param name Display name. @returns Canonical URL title; accents and punctuation do not distinguish routes. */
export function customHomeSlug(name: string): string {
  return name.normalize('NFKD').replace(/\p{M}/gu, '').toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, '_').replace(/^_+|_+$/g, '')
}

/** @param home Persisted definition with a stable internal id. @returns Public title-based destination. */
export function customHomeRoute(home: CustomHomeDefinition) {
  return { name: 'custom-home', params: { homeSlug: customHomeSlug(home.name) } }
}