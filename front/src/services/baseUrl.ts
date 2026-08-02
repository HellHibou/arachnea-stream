/**
 * Absolute URL of the directory the app is served from.
 *
 * Prefers the document base URL, which reflects the `<base href>` tag injected
 * by the server with the runtime mount point. This keeps router, API, and
 * locale resolution consistent with asset loading even on deep History-API
 * routes. Falls back to resolving Vite's configured base (e.g. `./`) against
 * the current page when no servable base tag is present (dev server, build
 * preview).
 *
 * @returns The app base directory URL, always ending with `/`.
 */
export function getAppBaseDir(): string {
  const documentBase = document.baseURI
  const hasReplacedBase = documentBase && !documentBase.includes('{base}')

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
 * @returns The app base path, e.g. `/arachnea-stream/`.
 */
export function getAppBasePath(): string {
  return new URL(getAppBaseDir()).pathname
}
