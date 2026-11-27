import { computed, onScopeDispose, ref, shallowRef, type Ref } from 'vue'
import { injectProfiles } from '@/composables/useProfiles'
import { isUncertainProfileWrite, profileCall, profileWrite, ProfileApiError, sameProfileValues, type ProfileSnapshot } from '@/services/profileApi'
import { isCurrentProfile, requireProfileContext } from '@/services/profileContext'
import { copyPreferences } from '@/services/profilePreferences'
import type { HomePreferences, Parameters } from '@/services/storage'
import { useLocalPlayerPreferences } from './useLocalPlayerPreferences'

/** Creates detached server/device drafts and reconciles uncertain server writes. */
export function usePreferenceDraft() {
  const profiles = injectProfiles()
  const context = requireProfileContext()
  const baseline = shallowRef<ProfileSnapshot | null>(null)
  const parameters = {} as Parameters
  const localPlayerPreferences = useLocalPlayerPreferences()
  const localBaseline = ref(localPlayerPreferences.cinemaModeByDefault.value)
  const cinemaModeByDefault = ref(localBaseline.value)
  const homePreferences: Pick<HomePreferences, 'showSectionEditingButtons'> = { showSectionEditingButtons: ref(true) }
  const loading = ref(false)
  const saving = ref(false)
  const errorCode = ref<string | null>(null)
  const blocked = ref(false)
  let disposed = false
  const current = () => !disposed && isCurrentProfile(context)
  const ready = computed(() => baseline.value !== null)

  /** Builds complete replacement documents, retaining fields outside this form. */
  function documents() {
    const source = baseline.value!
    const common = { language: parameters.language.value, theme: parameters.theme.value }
    const streaming = copyPreferences(source.streaming!.values)
    for (const key of Object.keys(parameters) as Array<keyof Parameters>) {
      if (Object.prototype.hasOwnProperty.call(streaming, key)) streaming[key] = copyPreferences(parameters[key].value)
    }
    streaming['videoPlayer.subtitleLanguages'] = copyPreferences(streaming['videoPlayer.subtitleLanguagePriority'])
    const priority = streaming['videoPlayer.subtitleLanguagePriority'] as string[]
    streaming['videoPlayer.subtitleTranslationByLanguage'] = Object.fromEntries(
      Object.entries(streaming['videoPlayer.subtitleTranslationByLanguage'] as Record<string, boolean>)
        .filter(([key]) => priority.includes(key)),
    )
    streaming['arachnea.home.showSectionEditingButtons'] = homePreferences.showSectionEditingButtons.value
    const trackKeys = ['videoPlayer.trackProfiles', 'videoPlayer.subtitleLanguagePriority', 'videoPlayer.subtitleTranslationByLanguage', 'videoPlayer.allowAudioDescription']
    if (trackKeys.some((key) => JSON.stringify(streaming[key]) !== JSON.stringify(source.streaming!.values[key]))) {
      streaming['arachnea.videoPlayer.preferences.audioTrack'] = null
      streaming['arachnea.videoPlayer.preferences.textTrack'] = { id: null, language: null, label: null, kind: null, mode: 'disabled', origin: 'profile' }
    }
    return { common, streaming }
  }

  const dirty = computed(() => {
    if (!baseline.value) return false
    const values = documents()
    return JSON.stringify(values.common) !== JSON.stringify(baseline.value.common.values)
      || JSON.stringify(values.streaming) !== JSON.stringify(baseline.value.streaming!.values)
      || cinemaModeByDefault.value !== localBaseline.value
  })

  /** Installs detached refs from an authoritative snapshot. @param source Completed server state. */
  function install(source: ProfileSnapshot): void {
    const values = { ...source.streaming!.values, ...source.common.values }
    for (const [key, value] of Object.entries(values)) {
      if (key.startsWith('arachnea.')) continue
      const refs = parameters as unknown as Record<string, Ref<unknown>>
      if (refs[key]) refs[key].value = copyPreferences(value)
      else refs[key] = ref(copyPreferences(value))
    }
    homePreferences.showSectionEditingButtons.value = source.streaming!.values['arachnea.home.showSectionEditingButtons'] as boolean
    baseline.value = source
  }

  /** Reads current revisions; optional explicit reapplication merges only the user's changed fields.
   * @param reapply Whether the user chose to reapply their edits after a conflict.
   */
  async function load(reapply = false): Promise<void> {
    if (loading.value || saving.value) return
    loading.value = true
    errorCode.value = null
    const previous = baseline.value
    const keepLocalEdit = reapply && cinemaModeByDefault.value !== localBaseline.value
    const edited = previous && reapply ? documents() : null
    try {
      const next = await profileCall<ProfileSnapshot>('get', {}, false, context)
      if (!current()) return
      if (!next.streaming) throw new ProfileApiError('invalid_data', 'Streaming preferences are not initialized.')
      install(next)
      localBaseline.value = localPlayerPreferences.cinemaModeByDefault.value
      if (!keepLocalEdit) cinemaModeByDefault.value = localBaseline.value
      if (edited && previous) {
        for (const key of Object.keys(parameters) as Array<keyof Parameters>) {
          const domain = key === 'language' || key === 'theme' ? 'common' : 'streaming'
          const oldValues = previous[domain]!.values as Record<string, unknown>
          const newValues = edited[domain] as Record<string, unknown>
          if (JSON.stringify(newValues[key]) !== JSON.stringify(oldValues[key])) parameters[key].value = copyPreferences(newValues[key]) as never
        }
        const key = 'arachnea.home.showSectionEditingButtons'
        if (edited.streaming[key] !== previous.streaming!.values[key]) homePreferences.showSectionEditingButtons.value = edited.streaming[key] as boolean
      }
      blocked.value = false
    } catch (error) {
      if (current()) errorCode.value = error instanceof ProfileApiError ? error.code : 'storage_unavailable'
    } finally { loading.value = false }
  }

  /** Commits the device-only layout default after a successful settings save. */
  function saveLocalPreferences(): void {
    localPlayerPreferences.setCinemaModeByDefault(cinemaModeByDefault.value)
    localBaseline.value = cinemaModeByDefault.value
  }

  /** Saves changed server domains, verifies them, then commits the separate local default. */
  async function save(): Promise<boolean> {
    if (!current() || saving.value || loading.value || blocked.value || !baseline.value) return false
    if (!dirty.value) return true
    const source = baseline.value
    const values = documents()
    const common = sameProfileValues(values.common, source.common.values) ? undefined
      : { expected_revision: source.common.revision, values: values.common }
    const streaming = sameProfileValues(values.streaming, source.streaming!.values) ? undefined
      : { expected_revision: source.streaming!.revision, values: values.streaming }
    if (!common && !streaming) {
      saveLocalPreferences()
      return true
    }
    saving.value = true
    errorCode.value = null
    try {
      try {
        await profileWrite('save_preferences', { common, streaming }, 'settings', context)
      } catch (error) {
        if (!current()) return false
        if (!isUncertainProfileWrite(error)) throw error
      }
      blocked.value = true
      const confirmed = await profileCall<ProfileSnapshot>('get', {}, false, context)
      if (!current()) return false
      if ((common && !sameProfileValues(confirmed.common.values, common.values))
        || (streaming && !sameProfileValues(confirmed.streaming?.values, streaming.values))) {
        throw new ProfileApiError('revision_conflict', 'Reload settings before applying the draft.')
      }
      await profiles.acceptSnapshot(confirmed, context)
      if (!current()) return false
      install(confirmed)
      saveLocalPreferences()
      blocked.value = false
      return true
    } catch (error) {
      if (current()) errorCode.value = error instanceof ProfileApiError ? error.code : 'storage_unavailable'
      return false
    } finally { saving.value = false }
  }

  /** Requests browser confirmation before discarding an edited or unacknowledged draft. @param event Browser unload event. */
  function protectUnload(event: BeforeUnloadEvent): void {
    if (dirty.value || blocked.value || saving.value) { event.preventDefault(); event.returnValue = '' }
  }
  window.addEventListener('beforeunload', protectUnload)
  onScopeDispose(() => { disposed = true; window.removeEventListener('beforeunload', protectUnload) })
  return { parameters, homePreferences, cinemaModeByDefault, ready, dirty, blocked, loading, saving, errorCode, load, save }
}
