import { computed, inject, onMounted, onScopeDispose, provide, shallowRef, type InjectionKey, type Ref } from 'vue'
import { useAccountSession } from '@/composables/useAccountSession'
import { setLanguage } from '@/i18n'
import { useStorage } from '@/services/storage'
import { applyTheme, findThemePreset } from '@/services/theme'
import { isSessionContentAllowed, sessionGeneration } from '@/services/sessionEvents'
import { createOperationId } from '@/services/operationId'
import { reportPersonalDataFailure } from '@/services/personalDataStatus'
import { setActiveProfileContext } from '@/services/profileContext'
import {
  pollProfileOperation,
  profileSessionCall,
  profileSessionWrite,
  ProfileApiError,
  type AvatarCatalogue,
  type CommonPreferences,
  type OnboardingView,
  type OperationView,
  type ProfileList,
  type ProfileSnapshot,
  type ProfileView,
} from '@/services/profileApi'
import {
  forgetLastProfileId,
  profileServerIdentity,
  readLastProfileId,
  rememberLastProfileId,
} from '@/services/profileResume'

/** Common onboarding scope owned by the profile store. */
const COMMON_SCOPE = 'common'
/** Streaming onboarding scope initialized on first service use. */
const STREAMING_SCOPE = 'arachnea-stream'
/** Delay before retrying an uncertain creation request. */
const CREATE_RETRY_DELAY_MS = 400
/** Maximum attempts for one creation request/operation identity. */
const CREATE_ATTEMPTS = 3
/** Native desktop event used to invalidate profiles across separate webviews. */
const PROFILE_CHANGED_EVENT = 'arachnea-profiles-changed'

/** Optional Tauri event bridge exposed by the desktop shell. */
interface NativeProfileEvents {
  listen: (name: string, listener: () => void) => Promise<() => void>
  emit: (name: string) => Promise<void>
}

/** Profile state and operations shared by the gate, onboarding and switcher. */
export interface ProfilesStore {
  list: Readonly<Ref<ProfileList | null>>
  snapshot: Readonly<Ref<ProfileSnapshot | null>>
  avatars: Readonly<Ref<AvatarCatalogue | null>>
  activeProfile: Readonly<Ref<ProfileView | null>>
  activeId: Readonly<Ref<string | null>>
  loading: Readonly<Ref<boolean>>
  busy: Readonly<Ref<boolean>>
  errorCode: Readonly<Ref<string | null>>
  initialized: Readonly<Ref<boolean>>
  hasProfiles: Readonly<Ref<boolean>>
  pendingOnly: Readonly<Ref<boolean>>
  initialize: () => Promise<void>
  retry: () => Promise<void>
  clearError: () => void
  select: (profileId: string) => Promise<void>
  createProfile: (input: CreateProfileInput) => Promise<CreateProfileResult | null>
  deleteProfile: (profile: ProfileView) => Promise<boolean>
}

/** Values collected by the common guide. */
export interface CreateProfileInput { name: string; avatarId: string; language: string | null; theme: string }
/** Created identity and its common-only snapshot. */
export interface CreateProfileResult { profileId: string; snapshot: ProfileSnapshot }

/** Injection key used by profile UI rendered below the gate. */
export const profilesKey: InjectionKey<ProfilesStore> = Symbol('arachnea-profiles')

/**
 * Creates one account-scoped profile session controller.
 *
 * The gate calls this once per authenticated session and provides the result
 * to onboarding, the switcher and the application shell.
 *
 * @returns Reactive profile state and lifecycle operations.
 */
export function useProfiles(): ProfilesStore {
  const accounts = useAccountSession()
  const list = shallowRef<ProfileList | null>(null)
  const snapshot = shallowRef<ProfileSnapshot | null>(null)
  const avatars = shallowRef<AvatarCatalogue | null>(null)
  const activeId = shallowRef<string | null>(null)
  const loading = shallowRef(true)
  const busy = shallowRef(false)
  const errorCode = shallowRef<string | null>(null)
  const initialized = shallowRef(false)
  const userId = shallowRef<string | null>(null)
  let lifecycle = 0
  let channel: BroadcastChannel | null = null
  let unlistenNative: (() => void) | undefined
  let disposed = false
  let revalidation = 0
  let pendingCreate: { operationId: string; body: string } | null = null
  const native = (window as unknown as { __TAURI__?: { event?: NativeProfileEvents } }).__TAURI__?.event

  const activeProfile = computed<ProfileView | null>(() => {
    if (!snapshot.value || !activeId.value) return null
    return snapshot.value.profile.profile_id === activeId.value ? snapshot.value.profile : null
  })
  const hasProfiles = computed(() => Boolean(list.value?.profiles.some((profile) => profile.state === 'active')))
  const pendingOnly = computed(() => Boolean(list.value && list.value.used_slots > 0 && !hasProfiles.value))

  /** Checks whether one async selection still owns the session and current lifecycle. @param token Lifecycle token. @param session Captured session generation. */
  function current(token: number, session: number): boolean {
    return !disposed && token === lifecycle && session === sessionGeneration() && isSessionContentAllowed()
  }

  /** Reads the complete profile list for the authenticated account. */
  function fetchList(): Promise<ProfileList> {
    return profileSessionCall<ProfileList>('list')
  }

  /** Reads one owned profile snapshot. @param profileId Authoritative profile UUID. */
  function fetchSnapshot(profileId: string): Promise<ProfileSnapshot> {
    return profileSessionCall<ProfileSnapshot>('get', { profile_id: profileId })
  }

  /** Marks one guide scope complete, reusing an interrupted operation and tolerating a concurrent completion. @param profileId Profile UUID. @param scope Guide namespace. @param revision Current guide revision. @param operationId Existing operation identity to resume. */
  async function completeGuide(profileId: string, scope: string, revision: number, operationId?: string): Promise<void> {
    try {
      await profileSessionWrite<OnboardingView>('complete_onboarding', { profile_id: profileId, scope, expected_revision: revision }, operationId)
    } catch (error) {
      if (error instanceof ProfileApiError && ['revision_conflict', 'operation_id_conflict'].includes(error.code)) {
        const confirmed = await fetchSnapshot(profileId).catch(() => null)
        const guide = confirmed?.onboarding.find((entry) => entry.scope === scope)
        if (confirmed && (!guide || guide.state === 'completed')) return
      }
      throw error
    }
  }

  /** Initializes the streaming document with server defaults at first use. @param profileId Profile UUID. */
  async function initializeStreaming(profileId: string): Promise<void> {
    try {
      const operation = await profileSessionWrite<OperationView>('initialize_streaming', { profile_id: profileId, values: {} })
      await settleOperation(operation)
    } catch (error) {
      // Another tab may have initialized the document between the snapshot read and this write.
      const confirmed = await fetchSnapshot(profileId).catch(() => null)
      if (confirmed?.streaming) return
      throw error
    }
  }

  /** Waits for a durable operation, failing with a retryable code instead of publishing a partial profile. @param operation Initial operation view. */
  async function settleOperation(operation: OperationView): Promise<OperationView> {
    if (operation.state === 'completed') return operation
    const settled = await pollProfileOperation(operation.operation_id)
    if (settled?.state === 'completed') return settled
    throw new ProfileApiError('operation_in_progress', 'The profile operation is still pending.')
  }

  /** Finishes interrupted initialization and guide markers before activation. @param source Snapshot read from the server. @param token Lifecycle token. @param session Captured session generation. */
  async function ensureReady(source: ProfileSnapshot, token: number, session: number): Promise<ProfileSnapshot> {
    let next = source
    const pending = next.pending_operation
    if (pending && pending.state !== 'completed') {
      if (pending.kind === 'initialize_streaming') {
        const resumed = await profileSessionWrite<OperationView>('initialize_streaming', { profile_id: next.profile.profile_id, values: {} }, pending.operation_id)
        await settleOperation(resumed)
      } else if (pending.kind === 'complete_onboarding') {
        const guide = next.onboarding.find((entry) => entry.state === 'pending')
        if (!guide) throw new ProfileApiError('operation_in_progress', 'The interrupted guide operation is incomplete.')
        await completeGuide(next.profile.profile_id, guide.scope, guide.revision, pending.operation_id)
      } else {
        const settled = await pollProfileOperation(pending.operation_id)
        if (settled?.state !== 'completed') throw new ProfileApiError('operation_in_progress', 'The profile operation is still pending.')
      }
      if (!current(token, session)) return next
      next = await fetchSnapshot(next.profile.profile_id)
      if (!current(token, session)) return next
    }

    const commonGuide = next.onboarding.find((guide) => guide.scope === COMMON_SCOPE)
    if (commonGuide?.state === 'pending') {
      await completeGuide(next.profile.profile_id, COMMON_SCOPE, commonGuide.revision)
      next = await fetchSnapshot(next.profile.profile_id)
      if (!current(token, session)) return next
    }

    if (!next.streaming) {
      await initializeStreaming(next.profile.profile_id)
      next = await fetchSnapshot(next.profile.profile_id)
      if (!current(token, session)) return next
    }

    const streamingGuide = next.onboarding.find((guide) => guide.scope === STREAMING_SCOPE)
    if (streamingGuide?.state === 'pending') {
      await completeGuide(next.profile.profile_id, STREAMING_SCOPE, streamingGuide.revision)
      next = await fetchSnapshot(next.profile.profile_id)
    }
    return next
  }

  /** Applies confirmed common preferences to i18n and the theme after activation. @param values Server-owned common values. */
  async function applyCommon(values: CommonPreferences): Promise<void> {
    const parameters = useStorage().getParameters()
    if (parameters.language.value !== values.language) await setLanguage(values.language)
    if (parameters.theme.value !== values.theme) parameters.theme.value = values.theme
    applyTheme(findThemePreset(values.theme))
  }

  /** Clears the active selection locally and on the server context. @param profileId Profile that became inaccessible, or undefined to forget the current resume key. */
  async function clearActive(profileId?: string): Promise<void> {
    activeId.value = null
    snapshot.value = null
    if (userId.value && (!profileId || readLastProfileId(userId.value) === profileId)) forgetLastProfileId(userId.value)
    await setActiveProfileContext(null)
  }

  /** Publishes a cross-tab/desktop invalidation after an identity change. */
  function broadcast(): void {
    channel?.postMessage('changed')
    void native?.emit(PROFILE_CHANGED_EVENT).catch(() => undefined)
  }

  /** Maps one failure to visible state and clears only the affected inaccessible selection. @param error Transport or domain error. @param profileId Profile targeted by the failed operation, when known. */
  async function handleError(error: unknown, profileId?: string): Promise<void> {
    if (error instanceof ProfileApiError && error.code === 'profile_context_changed') return
    errorCode.value = error instanceof ProfileApiError ? error.code : 'storage_unavailable'
    if (error instanceof ProfileApiError && ['profile_inaccessible', 'resource_not_found'].includes(error.code)) {
      if (!profileId || activeId.value === profileId) await clearActive(profileId).catch(() => undefined)
      else if (userId.value && readLastProfileId(userId.value) === profileId) forgetLastProfileId(userId.value)
      list.value = await fetchList().catch(() => list.value)
    }
  }

  /** Loads one profile, finishing pending guides, then mounts content under its context. @param profileId Selected profile UUID. @param session Captured session generation. @param token Lifecycle token. @param remember Whether to persist the selection locally. */
  async function selectInternal(profileId: string, session: number, token: number, remember: boolean): Promise<void> {
    const accountId = userId.value
    if (!accountId) return
    busy.value = true
    let published = false
    const previous = activeId.value
    if (previous && previous !== profileId) {
      activeId.value = null
      snapshot.value = null
      await setActiveProfileContext(null)
      if (!current(token, session)) return
    }
    try {
      let next = await fetchSnapshot(profileId)
      if (!current(token, session)) return
      next = await ensureReady(next, token, session)
      if (!current(token, session)) return
      await setActiveProfileContext(profileId)
      published = true
      if (!current(token, session)) return
      await applyCommon(next.common.values)
      await useStorage().refreshCachedRecords().catch(reportPersonalDataFailure)
      if (!current(token, session)) return
      activeId.value = profileId
      snapshot.value = next
      if (remember) rememberLastProfileId(accountId, profileId)
      errorCode.value = null
    } catch (error) {
      if (!current(token, session)) return
      if (published && activeId.value !== profileId) await setActiveProfileContext(null).catch(() => undefined)
      await handleError(error, profileId)
    } finally {
      busy.value = false
    }
  }

  /** Refreshes lists and validates the active profile without unmounting content. */
  async function revalidate(): Promise<void> {
    if (disposed || !initialized.value || busy.value || loading.value || !userId.value || !isSessionContentAllowed()) return
    const session = sessionGeneration()
    const token = lifecycle
    const request = ++revalidation
    try {
      const nextList = await fetchList()
      if (!current(token, session) || request !== revalidation) return
      list.value = nextList
      if (!activeId.value) return
      const selected = nextList.profiles.find((profile) => profile.profile_id === activeId.value && profile.state === 'active')
      if (!selected) {
        await clearActive(activeId.value ?? undefined)
        errorCode.value = 'profile_inaccessible'
        return
      }
      const next = await fetchSnapshot(selected.profile_id)
      if (!current(token, session) || request !== revalidation || activeId.value !== selected.profile_id) return
      snapshot.value = next
      await applyCommon(next.common.values)
    } catch (error) {
      if (current(token, session) && request === revalidation) await handleError(error)
    }
  }

  /** Restores the last profile or exposes the selector after listing. */
  async function initialize(): Promise<void> {
    const session = sessionGeneration()
    const token = ++lifecycle
    loading.value = true
    errorCode.value = null
    try {
      const accountId = accounts.session.value?.account?.user_id
      if (!accountId || !accounts.ready.value) return
      userId.value = accountId
      const [catalogue, nextList] = await Promise.all([profileSessionCall<AvatarCatalogue>('avatars'), fetchList()])
      if (!current(token, session)) return
      avatars.value = catalogue
      list.value = nextList
      const last = readLastProfileId(accountId)
      const restore = last ? nextList.profiles.find((profile) => profile.profile_id === last && profile.state === 'active') : undefined
      if (restore) await selectInternal(restore.profile_id, session, token, false)
    } catch (error) {
      if (current(token, session)) await handleError(error)
    } finally {
      if (current(token, session)) {
        loading.value = false
        initialized.value = true
      }
    }
  }

  /** Reloads the account state after an explicit error or interrupted start. */
  async function retry(): Promise<void> {
    await initialize()
  }

  /** Clears the last visible profile error when opening a fresh guide. */
  function clearError(): void {
    errorCode.value = null
  }

  /** Selects a profile explicitly from the switcher. @param profileId Selected profile UUID. */
  async function select(profileId: string): Promise<void> {
    const session = sessionGeneration()
    const token = ++lifecycle
    errorCode.value = null
    try {
      await selectInternal(profileId, session, token, true)
    } finally {
      if (current(token, session)) {
        loading.value = false
        initialized.value = true
      }
    }
  }

  /** Creates a common profile and completes its common guide, leaving streaming for first use. @param input Validated guide values. */
  async function createProfile(input: CreateProfileInput): Promise<CreateProfileResult | null> {
    const session = sessionGeneration()
    const token = lifecycle
    const accountId = userId.value
    if (!accountId) return null
    let createdId: string | null = null
    busy.value = true
    errorCode.value = null
    try {
      const body = {
        name: input.name,
        avatar_id: input.avatarId,
        common_values: { language: input.language, theme: input.theme },
      }
      const serialized = JSON.stringify(body)
      if (!pendingCreate || pendingCreate.body !== serialized) {
        pendingCreate = { operationId: createOperationId(), body: serialized }
      }
      const operationId = pendingCreate.operationId
      let operation: OperationView | null = null
      for (let attempt = 0; attempt < CREATE_ATTEMPTS; attempt += 1) {
        try {
          operation = await profileSessionWrite<OperationView>('create', body, operationId)
          break
        } catch (error) {
          const transient = !(error instanceof ProfileApiError) || error.code === 'storage_unavailable'
          if (!transient || attempt === CREATE_ATTEMPTS - 1) throw error
          await new Promise((resolve) => setTimeout(resolve, CREATE_RETRY_DELAY_MS * (attempt + 1)))
          if (!current(token, session)) return null
        }
      }
      if (!operation) throw new ProfileApiError('storage_unavailable', 'Profile creation is unavailable.')
      const settled = await settleOperation(operation)
      if (!current(token, session)) return null
      const profileId = typeof settled.result?.profile_id === 'string' ? settled.result.profile_id : null
      if (!profileId) throw new ProfileApiError('storage_unavailable', 'Profile creation did not return an identity.')
      createdId = profileId
      list.value = await fetchList()
      let next = await fetchSnapshot(profileId)
      const commonGuide = next.onboarding.find((guide) => guide.scope === COMMON_SCOPE)
      if (commonGuide?.state === 'pending') {
        await completeGuide(profileId, COMMON_SCOPE, commonGuide.revision)
        next = await fetchSnapshot(profileId)
      }
      rememberLastProfileId(accountId, profileId)
      broadcast()
      pendingCreate = null
      return { profileId, snapshot: next }
    } catch (error) {
      if (error instanceof ProfileApiError && !['storage_unavailable', 'operation_in_progress'].includes(error.code)) pendingCreate = null
      if (current(token, session)) await handleError(error, createdId ?? undefined)
      return null
    } finally {
      busy.value = false
    }
  }

  /** Deletes one owned profile and clears it when active. @param profile Selected profile view with its revision. @returns Whether the server confirmed deletion. */
  async function deleteProfile(profile: ProfileView): Promise<boolean> {
    const session = sessionGeneration()
    const token = lifecycle
    busy.value = true
    errorCode.value = null
    try {
      const operation = await profileSessionWrite<OperationView>('delete', { profile_id: profile.profile_id, expected_revision: profile.revision })
      if (!current(token, session)) return false
      // The server blocks the identity as soon as the deletion is accepted.
      if (activeId.value === profile.profile_id) {
        activeId.value = null
        snapshot.value = null
        await setActiveProfileContext(null)
      }
      if (userId.value && readLastProfileId(userId.value) === profile.profile_id) forgetLastProfileId(userId.value)
      let settleError: unknown = null
      try {
        await settleOperation(operation)
      } catch (error) {
        settleError = error
      }
      if (!current(token, session)) return false
      list.value = await fetchList()
      broadcast()
      if (settleError) {
        await handleError(settleError)
        return false
      }
      return true
    } catch (error) {
      await handleError(error)
      list.value = await fetchList().catch(() => list.value)
      return false
    } finally {
      busy.value = false
    }
  }

  /** Revalidates on window focus or when the document becomes visible. */
  function refresh(): void {
    if (document.visibilityState === 'visible') void revalidate()
  }

  onMounted(() => {
    window.addEventListener('focus', refresh)
    document.addEventListener('visibilitychange', refresh)
    if ('BroadcastChannel' in window) {
      channel = new BroadcastChannel(`arachnea.profiles.${profileServerIdentity()}`)
      channel.addEventListener('message', () => { void revalidate() })
    }
    if (native) {
      void native.listen(PROFILE_CHANGED_EVENT, () => { void revalidate() }).then((unlisten) => {
        if (disposed) unlisten()
        else unlistenNative = unlisten
      }).catch(() => undefined)
    }
    void initialize()
  })
  onScopeDispose(() => {
    disposed = true
    lifecycle += 1
    channel?.close()
    channel = null
    unlistenNative?.()
    unlistenNative = undefined
    window.removeEventListener('focus', refresh)
    document.removeEventListener('visibilitychange', refresh)
  })

  return {
    list,
    snapshot,
    avatars,
    activeProfile,
    activeId,
    loading,
    busy,
    errorCode,
    initialized,
    hasProfiles,
    pendingOnly,
    initialize,
    retry,
    clearError,
    select,
    createProfile,
    deleteProfile,
  }
}

/** Provides the profile session controller to the authenticated component tree. @param store Controller returned by {@link useProfiles}. */
export function provideProfiles(store: ProfilesStore): void {
  provide(profilesKey, store)
}

/** Returns the authenticated profile controller. */
export function injectProfiles(): ProfilesStore {
  const store = inject(profilesKey)
  if (!store) throw new Error('Profile session is unavailable.')
  return store
}
