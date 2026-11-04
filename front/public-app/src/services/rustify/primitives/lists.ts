/**
 * Alternates values from each source list while preserving each list's order.
 *
 * @param lists Ordered value lists, one for each source.
 * @returns Values interleaved by their position in each source list.
 */
export function interleaveLists<T>(lists: T[][]): T[] {
  const longestListLength = Math.max(0, ...lists.map((items) => items.length))
  const interleavedItems: T[] = []

  for (let itemIndex = 0; itemIndex < longestListLength; itemIndex += 1) {
    lists.forEach((items) => {
      const item = items[itemIndex]
      if (item !== undefined) {
        interleavedItems.push(item)
      }
    })
  }

  return interleavedItems
}
