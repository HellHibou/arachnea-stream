/** One independent desktop page reported by the native tab host. */
export interface DesktopTab {
  id: string
  title: string
}

/** Authoritative native tab state. */
export interface DesktopTabsSnapshot {
  tabs: DesktopTab[]
  active: string
}

/** Minimal API provided by Tauri's configured global bridge. */
interface DesktopWindow extends Window {
  __DESKTOP_TAB_SHELL__?: boolean
  __DESKTOP_TAB_PAGE__?: boolean
  __TAURI__?: {
    core: { invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T> }
    event: {
      listen: <T>(event: string, callback: (event: { payload: T }) => void) => Promise<() => void>
    }
  }
}

/** Returns whether this webview hosts the native desktop tab strip. */
export function isDesktopTabShell(): boolean {
  return (window as DesktopWindow).__DESKTOP_TAB_SHELL__ === true
}

/** Returns whether this webview renders a page managed by the desktop tab host. */
export function isDesktopTabPage(): boolean {
  return (window as DesktopWindow).__DESKTOP_TAB_PAGE__ === true
}

/**
 * Invokes a tab operation and returns the native snapshot.
 * @param action - Requested lifecycle operation.
 * @param id - Target tab, omitted for creation and initial state.
 */
export function requestDesktopTabs(
  action: 'snapshot' | 'open' | 'activate' | 'close',
  id?: string,
): Promise<DesktopTabsSnapshot> {
  const api = (window as DesktopWindow).__TAURI__
  if (!api) return Promise.reject(new Error('Desktop bridge is unavailable'))
  return api.core.invoke(`desktop_tabs_${action}`, { id })
}

/**
 * Reports document fullscreen state so the native host can resize its child webviews.
 * @param fullscreen - Whether this managed page currently owns document fullscreen.
 */
export function reportDesktopTabsFullscreen(fullscreen: boolean): Promise<void> {
  const api = (window as DesktopWindow).__TAURI__
  if (!api) return Promise.reject(new Error('Desktop bridge is unavailable'))
  return api.core.invoke('desktop_tabs_set_fullscreen', { fullscreen })
}

/**
 * Subscribes to native tab updates, including page title changes.
 * @param callback - Receives each authoritative snapshot.
 */
export function listenDesktopTabs(
  callback: (snapshot: DesktopTabsSnapshot) => void,
): Promise<() => void> {
  const api = (window as DesktopWindow).__TAURI__
  if (!api) return Promise.reject(new Error('Desktop bridge is unavailable'))
  return api.event.listen<DesktopTabsSnapshot>('desktop-tabs-changed', ({ payload }) => callback(payload))
}
