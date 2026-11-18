import { readonly, ref } from 'vue'
import { fetchPublicUsers, createPublicUser, resetPublicUserPassword, setPublicUserPasswordChange, deletePublicUser, AdminApiException, type PublicUser } from '@/services/adminApi'

/** Manages administrator user operations without storing credential drafts. */
export function useUserManagement() {
  const users = ref<PublicUser[]>([])
  const busy = ref(false)
  const errorCode = ref<string | null>(null)
  /** Refreshes safe account rows. */
  async function refresh(): Promise<void> { users.value = await fetchPublicUsers() }
  /** Runs one operation and refreshes confirmed state. @param action Authorized API mutation/read. @returns Whether the operation succeeded. */
  async function run(action: () => Promise<void>): Promise<boolean> {
    busy.value = true; errorCode.value = null
    try { await action(); await refresh(); return true }
    catch (error) { errorCode.value = error instanceof AdminApiException ? error.code : 'storage_unavailable'; return false }
    finally { busy.value = false }
  }
  return {
    users: readonly(users), busy: readonly(busy), errorCode: readonly(errorCode),
    load: () => run(async () => {}),
    /** Clears a previous error before opening a fresh form. */
    clearError: () => { errorCode.value = null },
    /** Creates a restricted account. @param id Retry UUID. @param login Login identifier. @param password Temporary password. @param mustChange Mandatory replacement at the next sign-in. */
    create: (id: string, login: string, password: string, mustChange: boolean) => run(() => createPublicUser(id, login, password, mustChange)),
    /** Resets a selected account. @param id Retry UUID. @param user Target account. @param password Temporary password. */
    reset: (id: string, user: string, password: string) => run(() => resetPublicUserPassword(id, user, password)),
    /** Updates the mandatory password replacement flag. @param id Retry UUID. @param user Target account. @param mustChange New flag value. */
    setPasswordChange: (id: string, user: string, mustChange: boolean) => run(() => setPublicUserPasswordChange(id, user, mustChange)),
    /** Deletes a confirmed account. @param id Retry UUID. @param user Target account. */
    remove: (id: string, user: string) => run(() => deletePublicUser(id, user)),
  }
}
