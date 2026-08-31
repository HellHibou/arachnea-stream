import type { LocationQuery, LocationQueryRaw } from 'vue-router'

/** Normalized search state extracted from or applied to route query parameters. */
export interface SearchRouteState {
  /** Search text query. */
  query: string
  /** Selected media type filters. */
  mediaTypes: string[]
  /** Selected theme filters. */
  themes: string[]
}

/**
 * Reads the search route query into normalized values consumed by the toolbar and search view.
 *
 * @param query Vue Router query object.
 * @returns Normalized search text and filters.
 */
export function readSearchRouteQuery(query: LocationQuery): SearchRouteState {
  return {
    query: readQueryString(query.q),
    mediaTypes: readQueryStringList(query.type),
    themes: readQueryStringList(query.themes),
  }
}

/**
 * Builds a route query object for the search screen.
 *
 * @param state Search text and filters selected in the toolbar.
 * @returns Vue Router query object with empty values omitted.
 */
export function buildSearchRouteQuery(state: SearchRouteState): LocationQueryRaw {
  const routeQuery: LocationQueryRaw = {}
  const query = state.query.trim()

  if (query) {
    routeQuery.q = query
  }

  if (state.mediaTypes.length > 0) {
    routeQuery.type = state.mediaTypes
  }

  if (state.themes.length > 0) {
    routeQuery.themes = state.themes
  }

  return routeQuery
}

/**
 * Normalizes a route query value into a trimmed string.
 *
 * @param value - Query parameter value which may be a string, string array, or undefined.
 * @returns First non-empty trimmed string, or empty string if not available.
 */
function readQueryString(value: LocationQuery[string] | undefined): string {
  if (Array.isArray(value)) {
    return value.find((item) => typeof item === 'string' && item.trim())?.trim() ?? ''
  }

  return typeof value === 'string' ? value.trim() : ''
}

/**
 * Normalizes a route query value into an array of unique trimmed strings.
 *
 * @param value - Query parameter value which may be a string, string array, or undefined.
 * @returns Array of unique non-empty trimmed strings.
 */
function readQueryStringList(value: LocationQuery[string] | undefined): string[] {
  const values = Array.isArray(value) ? value : [value]
  const uniqueValues = new Set<string>()

  values.forEach((item) => {
    if (typeof item !== 'string') {
      return
    }

    const normalizedValue = item.trim()
    if (normalizedValue) {
      uniqueValues.add(normalizedValue)
    }
  })

  return [...uniqueValues]
}
