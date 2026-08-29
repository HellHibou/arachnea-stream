/**
 * Returns whether the document base still carries the unreplaced `<base>`
 * marker, either literal or percent-encoded (browsers encode `{`/`}`).
 */
function hasUnresolvedBaseMarker(base: string): boolean {
  return base.includes('{base}') || base.includes('%7Bbase%7D')
}

/**
 * Absolute URL of the directory the app is served from.
 *
 * Prefers the document base URL, which reflects the `<base href>` tag injected
 * by the server with the runtime mount point. Falls back to resolving Vite's
 * configured base against the current page when no servable base tag is present.
 *
 * @returns The app base directory URL, always ending with `/`.
 */
export function getAppBaseDir(): string {
  const documentBase = document.baseURI
  const hasReplacedBase = documentBase && !hasUnresolvedBaseMarker(documentBase)

  if (hasReplacedBase) {
    return new URL('.', documentBase).href
  }

  return new URL(import.meta.env.BASE_URL, window.location.href).href
}

/**
 * Resolves a public-relative path against the app base directory.
 *
 * @param path - Path fragment relative to the app base directory.
 * @returns Absolute URL of the resolved path.
 */
export function resolveAppPath(path: string): string {
  return new URL(path, getAppBaseDir()).href
}

/**
 * Path-only base of the app (without origin), ending with `/`.
 *
 * Suitable for `createWebHistory`, which expects a path base rather than a
 * full URL.
 *
 * @returns The app base path, e.g. `/admin/`.
 */
export function getAppBasePath(): string {
  return new URL(getAppBaseDir()).pathname
}

/**
 * Builds the admin API URL from the public root.
 *
 * The API is served under `{root}/api/admin/<operation>` in HTTP mode.
 * This ensures the frontend works behind an entrypoint root.
 *
 * @param operation - Admin operation name (e.g., `status`, `services`).
 * @returns Absolute URL for the admin API endpoint.
 */
export function getAdminApiUrl(operation: string): string {
  return `${getAppBaseDir()}api/admin/${operation}`
}
