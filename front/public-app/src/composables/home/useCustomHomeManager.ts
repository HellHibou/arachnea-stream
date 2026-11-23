import { onScopeDispose, shallowRef } from 'vue'
import { createCustomHomeDraft, deleteCustomHome, invalidateHomesFetch, listCustomHomes, loadCustomHome, reorderCustomHomes, saveCustomHome } from '@/services/customHomes'
import { moveCustomHomeItem } from '@/services/customHomeDraft'
import type { CustomHomeDefinition } from '@/types/customHome'
import { t } from '@/i18n'
import { isUncertainProfileWrite, ProfileApiError } from '@/services/profileApi'
import { onWindowForeground } from '@/services/windowForeground'

interface CustomHomeManagerOptions {
  /** Gates automatic refreshes so navigation-owned instances stay idle while the sidebar is hidden. */
  shouldRefresh?: () => boolean
}

/** @param options Optional gate for automatic refreshes. @returns Home snapshots and persistence actions; failures never close or overwrite an editor draft. */
export function useCustomHomeManager(options: CustomHomeManagerOptions = {}) {
  /** Returns whether an automatic refresh is currently allowed. */
  function canAutoRefresh(): boolean { return options.shouldRefresh?.() ?? true }
  const homes = shallowRef<CustomHomeDefinition[]>([])
  const draft = shallowRef<CustomHomeDefinition | null>(null)
  const expectedRevision = shallowRef<number | null>(null)
  const errorMessage = shallowRef<string | null>(null)
  const busy = shallowRef(false)
  let request = 0

  /** Refreshes navigation snapshots, preserving the active draft even after external changes. */
  async function refresh(): Promise<void> {
    const revision = ++request
    try {
      const result = await listCustomHomes()
      if (revision !== request) return
      homes.value = result.homes
      if (draft.value && expectedRevision.value !== null &&
        result.homes.find((home) => home.id === draft.value?.id)?.updatedAt !== expectedRevision.value) {
        errorMessage.value = t('customHome.conflict')
      }
      if (!draft.value) errorMessage.value = result.issues.length ? result.issues.map((issue) => issue.message).join('\n') : null
    } catch (error) { if (revision === request) fail(error) }
  }
  /** @param error Storage or conflict failure retained for the user. */
  function fail(error: unknown): void {
    if (error instanceof ProfileApiError && ['profile_context_changed', 'personal_data_changed'].includes(error.code)) return
    errorMessage.value = error instanceof Error ? error.message : String(error)
  }
  /** @param id Explicit destination; omitted creates only a detached draft. @returns Prepared draft or null on failure. */
  async function begin(id?: string): Promise<CustomHomeDefinition | null> {
    if (busy.value) return null
    busy.value = true
    errorMessage.value = null
    try {
      const value = id ? await loadCustomHome(id) : createCustomHomeDraft('New home',
        Math.max(-1, ...homes.value.map((home) => home.navigationOrder)) + 1)
      if (!value) throw new Error(`Custom home no longer exists: ${id}`)
      expectedRevision.value = id ? value.updatedAt : null
      draft.value = value
      return value
    } catch (error) { fail(error); return null }
    finally { busy.value = false }
  }
  /** Refreshes the list before the user decides whether to submit a failed write again. */
  async function refreshAfterUncertainWrite(error: unknown): Promise<void> {
    if (isUncertainProfileWrite(error)) {
      invalidateHomesFetch()
      await refresh()
    }
    fail(error)
  }
  /** @param value Validated detached editor draft. @returns Committed definition, only after server confirmation. */
  async function save(value: CustomHomeDefinition): Promise<CustomHomeDefinition | null> {
    if (busy.value || value.id !== draft.value?.id) return null
    busy.value = true
    errorMessage.value = null
    try {
      const result = await saveCustomHome(value, expectedRevision.value)
      draft.value = null
      invalidateHomesFetch()
      window.dispatchEvent(new Event('custom-homes-changed'))
      await refresh()
      return result
    } catch (error) { await refreshAfterUncertainWrite(error); return null }
    finally { busy.value = false }
  }
  /** @param home Explicit confirmed deletion snapshot. @returns Whether deletion committed. */
  async function remove(home: CustomHomeDefinition): Promise<boolean> {
    if (busy.value) return false
    busy.value = true
    errorMessage.value = null
    try { await deleteCustomHome(home.id, home.updatedAt); invalidateHomesFetch(); window.dispatchEvent(new Event('custom-homes-changed')); await refresh(); return true }
    catch (error) { await refreshAfterUncertainWrite(error); return false }
    finally { busy.value = false }
  }
  /** @param index Selected home. @param direction Accessible navigation movement. */
  async function move(index: number, direction: -1 | 1): Promise<void> {
    if (busy.value) return
    busy.value = true
    errorMessage.value = null
    try { await reorderCustomHomes(moveCustomHomeItem(homes.value, index, direction)); invalidateHomesFetch(); window.dispatchEvent(new Event('custom-homes-changed')); await refresh() }
    catch (error) { await refreshAfterUncertainWrite(error) }
    finally { busy.value = false }
  }
  /** Discards local state without calling any persistence operation. */
  function cancel(): void { if (!busy.value) { draft.value = null; errorMessage.value = null } }
  /** Refreshes available destinations after another view may have saved a home. */
  function refreshWhenActive(): void { if (canAutoRefresh()) void refresh() }
  /** Drops the previous profile’s drafts and invalidates delayed replies. */
  function onProfile(): void { request++; homes.value = []; draft.value = null; expectedRevision.value = null; errorMessage.value = null; if (canAutoRefresh()) void refresh() }
  window.addEventListener('arachnea-profile-context', onProfile)
  const stopForeground = onWindowForeground(refreshWhenActive)
  window.addEventListener('custom-homes-changed', refreshWhenActive)
  onScopeDispose(() => {
    request++
    stopForeground()
    window.removeEventListener('arachnea-profile-context', onProfile)
    window.removeEventListener('custom-homes-changed', refreshWhenActive)
  })
  return { homes, draft, expectedRevision, errorMessage, busy, refresh, begin, save, remove, move, cancel }
}
