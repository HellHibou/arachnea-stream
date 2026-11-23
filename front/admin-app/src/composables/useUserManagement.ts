import { readonly, ref } from 'vue'
import { fetchPublicUsers, createPublicUser, resetPublicUserPassword, setPublicUserPasswordChange, deletePublicUser, AdminApiException, type PublicUser } from '@/services/adminApi'

/** Manages administrator user operations without storing credential drafts. */
export function useUserManagement() {
  const users = ref<PublicUser[]>([])
  const busy = ref(false)
  const errorCode = ref<string | null>(null)
  /** Refreshes safe account rows. */
  async function refresh(): Promise<void> { users.value = await fetchPublicUsers() }
  /** Reloads confirmed accounts after a write or an uncertain response. */
  async function run(action: () => Promise<void>, confirms?: (rows: PublicUser[]) => boolean): Promise<boolean> {
    if (busy.value) return false
    busy.value = true
    errorCode.value = null
    try {
      try {
        await action()
      } catch (error) {
        const code = error instanceof AdminApiException ? error.code : 'storage_unavailable'
        const uncertain = ['storage_unavailable', 'network_error', 'invalid_response', 'request_failed'].includes(code)
        if (uncertain) {
          const confirmed = await fetchPublicUsers().catch(() => null)
          if (confirmed) {
            users.value = confirmed
            if (confirms?.(confirmed)) return true
          }
        }
        errorCode.value = uncertain ? 'storage_unavailable' : code
        return false
      }
      await refresh().catch(() => { errorCode.value = 'refresh_failed' })
      return true
    } finally {
      busy.value = false
    }
  }
  return {
    users: readonly(users), busy: readonly(busy), errorCode: readonly(errorCode),
    load: async () => {
      if (busy.value) return
      busy.value = true
      errorCode.value = null
      try { await refresh() }
      catch (error) { errorCode.value = error instanceof AdminApiException ? error.code : 'refresh_failed' }
      finally { busy.value = false }
    },
    /** Clears a previous error before opening a fresh form. */
    clearError: () => { errorCode.value = null },
    /** Creates a restricted account. @param login Login identifier. @param password Temporary password. @param mustChange Mandatory replacement at the next sign-in. */
    create: (login: string, password: string, mustChange: boolean) => run(() => createPublicUser(login, password, mustChange)),
    /** Resets a selected account. @param user Target account. @param password Temporary password. */
    reset: (user: string, password: string) => run(() => resetPublicUserPassword(user, password)),
    /** Updates the mandatory password replacement flag. @param user Target account. @param mustChange New flag value. */
    setPasswordChange: (user: string, mustChange: boolean) => run(() => setPublicUserPasswordChange(user, mustChange), (rows) => rows.some((row) => row.user_id === user && row.must_change_password === mustChange)),
    /** Deletes a confirmed account. @param user Target account. */
    remove: (user: string) => run(() => deletePublicUser(user), (rows) => !rows.some((row) => row.user_id === user)),
  }
}
