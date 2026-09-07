import { onMounted, onUnmounted, readonly, shallowRef } from 'vue'
import { listenDesktopTabs, requestDesktopTabs, type DesktopTabsSnapshot } from '@/services/desktopTabs'

/** Owns tab subscriptions and forwards lifecycle actions to the native host. */
export function useDesktopTabs() {
  const state = shallowRef<DesktopTabsSnapshot>({ tabs: [], active: '' })
  const error = shallowRef('')
  let dispose: (() => void) | undefined
  let disposed = false

  /**
   * Executes a native action and exposes failures in the tab strip.
   * @param action - Lifecycle action requested by the user.
   * @param id - Existing tab to activate or close.
   */
  async function act(action: 'snapshot' | 'open' | 'activate' | 'close', id?: string) {
    try {
      error.value = ''
      await requestDesktopTabs(action, id)
    } catch (cause) {
      error.value = String(cause)
    }
  }

  onMounted(async () => {
    try {
      let receivedEvent = false
      dispose = await listenDesktopTabs((snapshot) => {
        receivedEvent = true
        state.value = snapshot
      })
      if (disposed) {
        dispose()
        return
      }
      const initial = await requestDesktopTabs('snapshot')
      if (!receivedEvent) state.value = initial
    } catch (cause) {
      error.value = String(cause)
    }
  })

  onUnmounted(() => {
    disposed = true
    dispose?.()
  })

  return { state: readonly(state), error: readonly(error), act }
}
