import { computed, onScopeDispose, ref, shallowRef, type Ref } from 'vue'
import { injectProfiles } from '@/composables/useProfiles'
import { createOperationId } from '@/services/operationId'
import { profileCall, ProfileApiError, type OperationView, type ProfileSnapshot } from '@/services/profileApi'
import { isCurrentProfile, requireProfileContext } from '@/services/profileContext'
import { copyPreferences } from '@/services/profilePreferences'
import type { HomePreferences, Parameters } from '@/services/storage'

/** Creates detached settings refs and retains one immutable save request until its outcome is known. */
export function usePreferenceDraft() {
  const profiles = injectProfiles()
  const context = requireProfileContext()
  const baseline = shallowRef<ProfileSnapshot | null>(null)
  const parameters = {} as Parameters
  const homePreferences: Pick<HomePreferences, 'showSectionEditingButtons'> = { showSectionEditingButtons: ref(true) }
  const loading = ref(false)
  const saving = ref(false)
  const errorCode = ref<string | null>(null)
  const pending = shallowRef<{ id: string; body: object } | null>(null)
  const operation = shallowRef<OperationView | null>(null)
  let disposed = false
  const current = () => !disposed && isCurrentProfile(context)
  const ready = computed(() => baseline.value !== null)
  const blocked = computed(() => pending.value !== null || Boolean(baseline.value?.pending_operation))

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
    operation.value = source.pending_operation
  }

  /** Reads current revisions; optional explicit reapplication merges only the user's changed fields.
   * @param reapply Whether the user chose to reapply their edits after a conflict.
   */
  async function load(reapply = false): Promise<void> {
    if (loading.value || saving.value || pending.value) return
    loading.value = true
    errorCode.value = null
    const previous = baseline.value
    const edited = previous && reapply ? documents() : null
    try {
      const next = await profileCall<ProfileSnapshot>('get', {}, false, context)
      if (!current()) return
      if (!next.streaming) throw new ProfileApiError('invalid_data', 'Streaming preferences are not initialized.')
      install(next)
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
      if (next.pending_operation) errorCode.value = 'operation_in_progress'
    } catch (error) {
      if (current()) errorCode.value = error instanceof ProfileApiError ? error.code : 'storage_unavailable'
    } finally { loading.value = false }
  }

  /** Saves changed domains once, or repeats the identical request after an uncertain/partial result. */
  async function save(): Promise<boolean> {
    if (!current() || saving.value || loading.value || !baseline.value || baseline.value.pending_operation) return false
    if (!pending.value && !dirty.value) return true
    if (!pending.value) {
      const values = documents()
      const body: Record<string, unknown> = {}
      if (JSON.stringify(values.common) !== JSON.stringify(baseline.value.common.values)) body.common = { expected_revision: baseline.value.common.revision, values: values.common }
      if (JSON.stringify(values.streaming) !== JSON.stringify(baseline.value.streaming!.values)) body.streaming = { expected_revision: baseline.value.streaming!.revision, values: values.streaming }
      pending.value = { id: createOperationId(), body }
    }
    saving.value = true
    errorCode.value = null
    try {
      const request = pending.value
      operation.value = await profileCall<OperationView>('save_preferences', { ...request.body, operation_id: request.id }, true, context)
      if (!current()) return false
      if (operation.value.state !== 'completed') {
        errorCode.value = 'operation_in_progress'
        return false
      }
      const confirmed = await profileCall<ProfileSnapshot>('get', {}, false, context)
      if (!current()) return false
      if (confirmed.pending_operation) { errorCode.value = 'operation_in_progress'; return false }
      await profiles.acceptSnapshot(confirmed, context)
      if (!current()) return false
      install(confirmed)
      pending.value = null
      return true
    } catch (error) {
      if (current()) {
        errorCode.value = error instanceof ProfileApiError ? error.code : 'storage_unavailable'
        if (error instanceof ProfileApiError && ['revision_conflict', 'invalid_data', 'unsupported_schema_version', 'operation_id_conflict'].includes(error.code)) pending.value = null
        if (pending.value) {
          try {
            const status = await profileCall<OperationView>('operation', { operation_id: pending.value.id }, false, context)
            if (current()) operation.value = status
          } catch { /* The retained request remains safe to repeat even when status is unavailable. */ }
        }
      }
      return false
    } finally { saving.value = false }
  }

  /** Requests browser confirmation before discarding an edited or unacknowledged draft. @param event Browser unload event. */
  function protectUnload(event: BeforeUnloadEvent): void {
    if (dirty.value || blocked.value || saving.value) { event.preventDefault(); event.returnValue = '' }
  }
  window.addEventListener('beforeunload', protectUnload)
  onScopeDispose(() => { disposed = true; window.removeEventListener('beforeunload', protectUnload) })
  return { parameters, homePreferences, ready, dirty, blocked, loading, saving, errorCode, operation, load, save }
}
