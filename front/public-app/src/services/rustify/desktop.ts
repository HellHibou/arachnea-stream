/**
 * Window type extension for Tauri-specific API access.
 */
export interface TauriWindow extends Window {
  /** Tauri-specific global object. */
  __TAURI__?: {
    /** Tauri invoke function (legacy path). */
    invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>
    /** Tauri core module. */
    core?: {
      /** Tauri invoke function (modern path). */
      invoke?: <T>(command: string, args?: Record<string, unknown>) => Promise<T>
    }
  }
}

/** Returns whether the application is running in a Tauri desktop window. */
export function isDesktopApp(): boolean {
  const tauriWindow = window as TauriWindow
  return Boolean(tauriWindow.__TAURI__?.core?.invoke ?? tauriWindow.__TAURI__?.invoke)
}

/** Opens or focuses the dedicated administration window in desktop mode. */
export async function openAdministrationWindow(): Promise<void> {
  const tauriWindow = window as TauriWindow
  const tauriInvoke = tauriWindow.__TAURI__?.core?.invoke ?? tauriWindow.__TAURI__?.invoke

  if (!tauriInvoke) {
    return
  }

  await tauriInvoke('open_admin_window')
}
