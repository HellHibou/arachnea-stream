import { computed, readonly, ref } from 'vue'

import { useAppConfig } from '@/services/appConfig'
import { fetchPublicUsers, fetchServices, fetchStatus, type ServicesResponse } from '@/services/adminApi'

/** Loads independent dashboard sources without treating failed requests as empty data. */
export function useDashboard() {
  const { serviceStores, serviceEmptyStates } = useAppConfig()
  const catalog = ref<ServicesResponse | null>(null)
  const passwordConfigured = ref<boolean | null>(null)
  const hasUsers = ref<boolean | null>(null)
  const loading = ref(false)
  const loadFailed = ref(false)

  const groups = computed(() => serviceStores.value.map((id) => {
    const store = catalog.value?.service_stores.find((group) => group.service_store_id === id)
    const enabled = store?.services.filter((service) => service.enabled).length ?? 0
    // Failed YAML sources are absent from services; count them once in the total.
    const unavailable = store?.unavailable.filter((source) =>
      !source.id || !store.services.some((service) => service.id === source.id),
    ).length ?? 0
    const emptyState = serviceEmptyStates.value[id]
    return {
      id,
      available: store !== undefined,
      enabled,
      total: (store?.services.length ?? 0) + unavailable,
      severity: enabled > 0 ? 'success' as const : (emptyState?.severity ?? 'info'),
      messageKey: enabled === 0 ? emptyState?.label : undefined,
    }
  }))

  /** Refreshes all sources, retaining no stale success or warning state on failure. */
  async function refresh(): Promise<void> {
    if (loading.value) return
    loading.value = true
    const [services, status, users] = await Promise.allSettled([
      fetchServices(), fetchStatus(), fetchPublicUsers(),
    ])
    catalog.value = services.status === 'fulfilled' ? services.value : null
    passwordConfigured.value = status.status === 'fulfilled' ? status.value.password_configured : null
    hasUsers.value = users.status === 'fulfilled' ? users.value.length > 0 : null
    loadFailed.value = [services, status, users].some((result) => result.status === 'rejected')
      || groups.value.some((group) => !group.available)
    loading.value = false
  }

  return {
    groups,
    passwordConfigured: readonly(passwordConfigured),
    hasUsers: readonly(hasUsers),
    loading: readonly(loading),
    loadFailed: readonly(loadFailed),
    refresh,
  }
}
