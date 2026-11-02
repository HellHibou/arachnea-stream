import { onUnmounted, ref } from 'vue'
import { createProxyKey, deleteProxyKey, fetchSettings, type ProxyKeyInfo } from '@/services/adminApi'
import { useI18n } from '@/i18n'

/** Owns one-time key secrets and asynchronous key mutations. */
export function useProxyApiKeys() {
  const { t } = useI18n()
  const keys = ref<ProxyKeyInfo[]>([])
  const token = ref('')
  const loading = ref(false)
  const error = ref('')

  async function mutate(action: () => Promise<void>): Promise<void> {
    if (loading.value) return
    loading.value = true
    error.value = ''
    try {
      await action()
      const settings = await fetchSettings()
      keys.value = settings.proxy?.keys ?? []
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : t('error.unknown')
    } finally {
      loading.value = false
    }
  }

  async function create(label: string): Promise<void> {
    token.value = ''
    await mutate(async () => {
      const result = await createProxyKey(label)
      token.value = result.token
    })
  }

  async function revoke(id: string): Promise<void> {
    await mutate(() => deleteProxyKey(id))
  }

  async function copy(): Promise<void> {
    try {
      await navigator.clipboard.writeText(token.value)
    } catch {
      error.value = t('proxy.copyFailed')
    }
  }

  onUnmounted(() => { token.value = '' })
  return { keys, token, loading, error, create, revoke, copy }
}
