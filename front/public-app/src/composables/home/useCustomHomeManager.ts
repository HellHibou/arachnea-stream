import { onScopeDispose, shallowRef } from 'vue'
import { createCustomHomeDraft, deleteCustomHome, listCustomHomes, loadCustomHome, reorderCustomHomes, saveCustomHome } from '@/services/customHomes'
import { moveCustomHomeItem } from '@/services/customHomeDraft'
import type { CustomHomeDefinition } from '@/types/customHome'
import { t } from '@/i18n'

/** @returns Home snapshots and persistence actions; failures never close or overwrite an editor draft. */
export function useCustomHomeManager() {
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
  function fail(error: unknown): void { errorMessage.value = error instanceof Error ? error.message : String(error) }
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
  /** @param value Validated detached editor draft. @returns Committed definition, only after IndexedDB completion. */
  async function save(value: CustomHomeDefinition): Promise<CustomHomeDefinition | null> {
    if (busy.value || value.id !== draft.value?.id) return null
    busy.value = true
    errorMessage.value = null
    try {
      const result = await saveCustomHome(value, expectedRevision.value)
      draft.value = null
      window.dispatchEvent(new Event('custom-homes-changed'))
      await refresh()
      return result
    } catch (error) { fail(error); return null }
    finally { busy.value = false }
  }
  /** @param home Explicit confirmed deletion snapshot. @returns Whether deletion committed. */
  async function remove(home: CustomHomeDefinition): Promise<boolean> {
    if (busy.value) return false
    busy.value = true
    errorMessage.value = null
    try { await deleteCustomHome(home.id, home.updatedAt); window.dispatchEvent(new Event('custom-homes-changed')); await refresh(); return true }
    catch (error) { fail(error); return false }
    finally { busy.value = false }
  }
  /** @param index Selected home. @param direction Accessible navigation movement. */
  async function move(index: number, direction: -1 | 1): Promise<void> {
    if (busy.value) return
    busy.value = true
    errorMessage.value = null
    try { await reorderCustomHomes(moveCustomHomeItem(homes.value, index, direction)); window.dispatchEvent(new Event('custom-homes-changed')); await refresh() }
    catch (error) { fail(error) }
    finally { busy.value = false }
  }
  /** Discards local state without calling any persistence operation. */
  function cancel(): void { if (!busy.value) { draft.value = null; errorMessage.value = null } }
  /** Refreshes available destinations after another view may have saved a home. */
  function onFocus(): void { void refresh() }
  /** Refreshes snapshots after returning to a visible tab without replacing a draft. */
  function onVisibility(): void { if (document.visibilityState === 'visible') void refresh() }
  window.addEventListener('focus', onFocus)
  window.addEventListener('custom-homes-changed', onFocus)
  document.addEventListener('visibilitychange', onVisibility)
  onScopeDispose(() => { request++; window.removeEventListener('focus', onFocus); window.removeEventListener('custom-homes-changed', onFocus); document.removeEventListener('visibilitychange', onVisibility) })
  return { homes, draft, expectedRevision, errorMessage, busy, refresh, begin, save, remove, move, cancel }
}