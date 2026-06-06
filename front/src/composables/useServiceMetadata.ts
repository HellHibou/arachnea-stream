import { computed, readonly, shallowRef } from 'vue'

import { t } from '@/i18n'
import { loadServiceMetadata } from '@/services/rustify'
import type { ServiceMetadata } from '@/types/serviceMetadata'

const services = shallowRef<ServiceMetadata[]>([])
const isLoading = shallowRef(false)
const errorMessage = shallowRef<string | null>(null)

const servicesById = computed(() => {
  const registry = new Map<string, ServiceMetadata>()

  services.value.forEach((service) => {
    registry.set(service.id, service)
  })

  return registry
})

/**
 * Provides the session-scoped registry of backend service display metadata.
 */
export function useServiceMetadata() {
  /**
   * Loads service metadata once for the current app session.
   *
   * @returns Loaded metadata rows.
   */
  async function load(): Promise<ServiceMetadata[]> {
    if (services.value.length > 0) {
      return services.value
    }

    isLoading.value = true
    errorMessage.value = null

    try {
      const loadedServices = await loadServiceMetadata()
      services.value = loadedServices
      return loadedServices
    } catch (error) {
      errorMessage.value =
        error instanceof Error
          ? error.message
          : t('errors.serviceMetadata')
      return []
    } finally {
      isLoading.value = false
    }
  }

  /**
   * Resolves one backend source id into display metadata.
   *
   * @param source Backend source id attached to a media item.
   * @returns Matching metadata, or `null` when unavailable.
   */
  function getService(source: string | null | undefined): ServiceMetadata | null {
    const serviceId = source?.trim()
    return serviceId ? servicesById.value.get(serviceId) ?? null : null
  }

  return {
    services: readonly(services),
    servicesById,
    isLoading: readonly(isLoading),
    errorMessage: readonly(errorMessage),
    load,
    getService,
  }
}
