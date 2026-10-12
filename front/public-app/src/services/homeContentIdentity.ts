import type { HomeContentIdentity } from '@/types/customHome'

/**
 * Builds an exact identity without array indexes or presentation slugification.
 * @param sourceId Backend source id.
 * @param providerKey Explicit YAML/provider key or id.
 * @param descriptor Persistent request descriptor, excluding pagination.
 * @param label Last-resort visible title; fragile references require repair when it changes.
 * @returns Identity metadata, or undefined when no reference can be built.
 */
export function buildHomeContentIdentity(
  sourceId: string | null,
  providerKey: string | null,
  descriptor: Record<string, string>,
  label: string | null,
): HomeContentIdentity | undefined {
  if (!sourceId) return undefined
  if (providerKey) {
    return { sourceId, key: `provider:${providerKey}`, basis: 'provider', ambiguous: false }
  }
  const entries = Object.entries(descriptor)
    .filter(([key]) => !['name', 'source', 'page', 'current_page', 'have_more'].includes(key))
    .sort(([left], [right]) => left.localeCompare(right))
  if (entries.length > 0) {
    return {
      sourceId,
      key: `descriptor:${JSON.stringify(entries)}`,
      basis: 'descriptor',
      ambiguous: false,
    }
  }
  return label
    ? { sourceId, key: `label:${label}`, basis: 'label', ambiguous: false }
    : undefined
}

/**
 * Marks collisions instead of silently resolving a saved reference to the first result.
 * @param entries Unmerged entries in the same reference scope.
 */
export function markAmbiguousHomeIdentities(
  entries: { identity?: HomeContentIdentity }[],
): void {
  const counts = new Map<string, number>()
  for (const entry of entries) {
    if (!entry.identity) continue
    const key = JSON.stringify([entry.identity.sourceId, entry.identity.key])
    counts.set(key, (counts.get(key) ?? 0) + 1)
  }
  for (const entry of entries) {
    if (!entry.identity) continue
    const key = JSON.stringify([entry.identity.sourceId, entry.identity.key])
    entry.identity.ambiguous = (counts.get(key) ?? 0) > 1
  }
}