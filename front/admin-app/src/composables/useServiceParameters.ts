import { computed, ref, shallowRef } from 'vue'
import { useAdminApi } from '@/composables/useAdminApi'
import { useI18n } from '@/i18n'
import type { ServiceParameter } from '@/services/adminApi'

/** Editor draft; restoring defaults stays cancellable until Save. */
export function useServiceParameters() {
  const api = useAdminApi()
  const { t } = useI18n()
  const parameters = ref<ServiceParameter[]>([])
  const values = ref<Record<string, string>>({})
  const loaded = shallowRef(false)
  let requestId = 0

  function reset() {
    requestId += 1
    loaded.value = false
    parameters.value = []
    values.value = {}
    api.clearError()
  }

  async function load(store: string, service: string) {
    reset()
    const currentRequest = requestId
    const result = await api.getServiceParameters(store, service)
    if (currentRequest !== requestId || !result) return
    parameters.value = result.parameters
    values.value = Object.fromEntries(result.parameters.map((parameter) => [parameter.name, parameter.value]))
    loaded.value = true
  }

  function validate(parameter: ServiceParameter, value: string): true | string {
    const editor = parameter.editor
    if (editor.type === 'select') {
      return editor.options.some((option) => option.value === value) || t('serviceParameters.invalid')
    }
    try {
      const match = new RegExp(`^(?:${editor.pattern})$`).exec(value)
      if (!match || match[0].length !== value.length) return editor.message
      if (parameter.name === 'base_url') {
        const url = new URL(value)
        if (!['http:', 'https:'].includes(url.protocol) || !url.hostname || url.username || url.password
          || url.search || url.hash || value.endsWith('/') || /\s/.test(value)) return editor.message
      }
      return true
    } catch {
      return editor.message
    }
  }

  const canSave = computed(() => loaded.value && parameters.value.length > 0
    && parameters.value.every((parameter) => validate(parameter, values.value[parameter.name] ?? '') === true))

  function restoreDefaults() {
    values.value = Object.fromEntries(parameters.value.map((parameter) => [parameter.name, parameter.default_value]))
  }

  async function save(store: string, service: string) {
    if (!canSave.value || api.isLoading.value) return false
    return !!await api.saveServiceParameters(store, service, { ...values.value })
  }

  return { parameters, values, loaded, canSave, load, save, reset, restoreDefaults, validate,
    isLoading: api.isLoading, error: api.error }
}
