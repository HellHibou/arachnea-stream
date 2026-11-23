import { computed, inject, onMounted, onScopeDispose, provide, shallowRef, type InjectionKey, type Ref } from 'vue'
import { useAccountSession } from '@/composables/useAccountSession'
import { useStorage } from '@/services/storage'
import { applyProfilePreferences } from '@/services/profilePreferences'
import { isSessionContentAllowed, sessionGeneration } from '@/services/sessionEvents'
import { reportPersonalDataFailure } from '@/services/personalDataStatus'
import { onWindowForeground } from '@/services/windowForeground'
import { requireProfileContext, isCurrentProfile, type ProfileContext, setActiveProfileContext } from '@/services/profileContext'
import {
  isUncertainProfileWrite,
  profileSessionCall,
  profileSessionWrite,
  ProfileApiError,
  type AvatarCatalogue,
  type OnboardingView,
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
  initialize: () => Promise<void>
  retry: () => Promise<void>
  acceptSnapshot: (value: ProfileSnapshot, context: ProfileContext) => Promise<void>
  clearError: () => void
  select: (profileId: string) => Promise<void>
  createProfile: (input: CreateProfileInput) => Promise<CreateProfileResult | null>
  updateProfile: (profile: ProfileView, name: string, avatarId: string) => Promise<boolean>
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
  const native = (window as unknown as { __TAURI__?: { event?: NativeProfileEvents } }).__TAURI__?.event

  const activeProfile = computed<ProfileView | null>(() => {
    if (!snapshot.value || !activeId.value) return null
    return snapshot.value.profile.profile_id === activeId.value ? snapshot.value.profile : null
  })
  const hasProfiles = computed(() => Boolean(list.value?.profiles.some((profile) => profile.state === 'active')))

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

  /** Marks one guide scope complete, tolerating a concurrent completion or a lost acknowledgement. @param profileId Profile UUID. @param scope Guide namespace. @param revision Current guide revision. */
  async function completeGuide(profileId: string, scope: string, revision: number): Promise<void> {
    try {
      await profileSessionWrite<OnboardingView>('complete_onboarding', { profile_id: profileId, scope, expected_revision: revision })
    } catch (error) {
      if (isUncertainProfileWrite(error) || (error instanceof ProfileApiError && error.code === 'revision_conflict')) {
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
      await profileSessionWrite('initialize_streaming', { profile_id: profileId, values: {} })
    } catch (error) {
      // Another tab may have initialized the document between the snapshot read and this write.
      const confirmed = await fetchSnapshot(profileId).catch(() => null)
      if (confirmed?.streaming) return
      throw error
    }
  }

  /** Completes initialization and pending guides before activation. @param source Snapshot read from the server. @param token Lifecycle token. @param session Captured session generation. */
  async function ensureReady(source: ProfileSnapshot, token: number, session: number): Promise<ProfileSnapshot> {
    let next = source
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

  /** Publishes confirmed preferences and invalidates other views after a save.
   * @param value Completed server snapshot.
   * @param context Generation captured by the settings editor.
   */
  async function acceptSnapshot(value: ProfileSnapshot, context: ProfileContext): Promise<void> {
    if (!isCurrentProfile(context) || value.profile.profile_id !== context.profileId) return
    revalidation += 1
    await applyProfilePreferences(value, context)
    if (!isCurrentProfile(context)) return
    snapshot.value = value
    broadcast()
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
      await applyProfilePreferences(next, requireProfileContext())
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
      await applyProfilePreferences(next, requireProfileContext())
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
      const activeProfiles = nextList.profiles.filter((profile) => profile.state === 'active')
      const askedAuthentication = accounts.justLoggedIn.value
      accounts.consumeJustLoggedIn?.()
      // Only show the selector after an explicit login; otherwise keep the last loaded profile.
      const shouldSelectManually = activeProfiles.length > 1 && askedAuthentication
      if (!shouldSelectManually) {
        const last = readLastProfileId(accountId)
        const restore = last ? nextList.profiles.find((profile) => profile.profile_id === last && profile.state === 'active') : undefined
        if (restore) await selectInternal(restore.profile_id, session, token, false)
      }
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
    if (busy.value || disposed || !isSessionContentAllowed()) return
    if (activeProfile.value?.profile_id === profileId) {
      await revalidate()
      return
    }
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
    if (!accountId || busy.value) return null
    let createdId: string | null = null
    busy.value = true
    errorCode.value = null
    try {
      const body = {
        name: input.name,
        avatar_id: input.avatarId,
        common_values: { language: input.language, theme: input.theme },
      }
      const { profile_id: profileId } = await profileSessionWrite<{ profile_id: string }>('create', body)
      if (!current(token, session)) return null
      createdId = profileId
      const confirmedList = await fetchList()
      if (!current(token, session)) return null
      list.value = confirmedList
      let next = await fetchSnapshot(profileId)
      if (!current(token, session)) return null
      const commonGuide = next.onboarding.find((guide) => guide.scope === COMMON_SCOPE)
      if (commonGuide?.state === 'pending') {
        await completeGuide(profileId, COMMON_SCOPE, commonGuide.revision)
        next = await fetchSnapshot(profileId)
      }
      if (!current(token, session)) return null
      rememberLastProfileId(accountId, profileId)
      broadcast()
      return { profileId, snapshot: next }
    } catch (error) {
      if (current(token, session)) {
        if (createdId || isUncertainProfileWrite(error)) {
          const confirmed = await fetchList().catch(() => null)
          if (!current(token, session)) return null
          if (confirmed) list.value = confirmed
        }
        await handleError(error, createdId ?? undefined)
      }
      return null
    } finally {
      busy.value = false
    }
  }

  /** Updates an owned identity without changing the active context.
   * @param profile Profile and revision captured when opening the editor.
   * @param name Edited display name.
   * @param avatarId Selected catalogue avatar.
   * @returns Whether the server confirmed the update.
   */
  async function updateProfile(profile: ProfileView, name: string, avatarId: string): Promise<boolean> {
    if (busy.value || disposed || !isSessionContentAllowed()) return false
    const session = sessionGeneration()
    const token = lifecycle
    const body = { profile_id: profile.profile_id, expected_revision: profile.revision, name: name.trim(), avatar_id: avatarId }
    busy.value = true
    errorCode.value = null
    revalidation += 1
    try {
      let updated: ProfileView
      try {
        updated = await profileSessionWrite<ProfileView>('update', body)
      } catch (error) {
        if (!isUncertainProfileWrite(error)) throw error
        const next = await fetchList().catch(() => null)
        if (!current(token, session)) return false
        if (next) list.value = next
        const confirmed = next?.profiles.find((entry) => entry.profile_id === profile.profile_id)
        if (!confirmed || confirmed.name !== body.name || confirmed.avatar_id !== avatarId) throw error
        updated = confirmed
      }
      if (!current(token, session)) return false
      revalidation += 1
      if (list.value) list.value = { ...list.value, profiles: list.value.profiles.map((entry) => entry.profile_id === updated.profile_id ? updated : entry) }
      if (snapshot.value?.profile.profile_id === updated.profile_id) snapshot.value = { ...snapshot.value, profile: updated }
      broadcast()
      return true
    } catch (error) {
      if (!current(token, session)) return false
      await handleError(error, profile.profile_id)
      if (error instanceof ProfileApiError && error.code === 'revision_conflict') {
        const nextList = await fetchList().catch(() => null)
        if (current(token, session) && nextList) list.value = nextList
      }
      return false
    } finally {
      busy.value = false
    }
  }

  /** Deletes one owned profile and clears it when active. @param profile Selected profile view with its revision. @returns Whether the server confirmed deletion. */
  async function deleteProfile(profile: ProfileView): Promise<boolean> {
    if (busy.value || disposed || !isSessionContentAllowed()) return false
    const session = sessionGeneration()
    const token = lifecycle
    busy.value = true
    errorCode.value = null
    try {
      try {
        await profileSessionWrite('delete', { profile_id: profile.profile_id, expected_revision: profile.revision })
      } catch (error) {
        if (!current(token, session)) return false
        const next = await fetchList().catch(() => null)
        if (!current(token, session)) return false
        if (next) list.value = next
        if (!next || next.profiles.some((entry) => entry.profile_id === profile.profile_id)) {
          await handleError(error, profile.profile_id)
          return false
        }
      }
      if (!current(token, session)) return false
      if (activeId.value === profile.profile_id) await clearActive(profile.profile_id)
      if (!current(token, session)) return false
      if (userId.value && readLastProfileId(userId.value) === profile.profile_id) forgetLastProfileId(userId.value)
      if (list.value) {
        const profiles = list.value.profiles.filter((entry) => entry.profile_id !== profile.profile_id)
        list.value = { ...list.value, profiles, used_slots: profiles.length }
      }
      broadcast()
      return true
    } finally {
      busy.value = false
    }
  }

  let stopForeground: (() => void) | undefined
  onMounted(() => {
    stopForeground = onWindowForeground(() => { void revalidate() })
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
    stopForeground?.()
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
    initialize,
    retry,
    acceptSnapshot,
    clearError,
    select,
    createProfile,
    updateProfile,
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
