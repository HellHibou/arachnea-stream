import { computed, readonly, shallowRef } from 'vue'

import { t } from '@/i18n'
import { loadServiceMetadata } from '@/services/rustify'
import type { ServiceMetadata } from '@/types/serviceMetadata'

/** Reactive array of loaded service metadata. */
const services = shallowRef<ServiceMetadata[]>([])
/** Loading state for service metadata. */
const isLoading = shallowRef(false)
/** Error message from service metadata loading, or null if successful. */
const errorMessage = shallowRef<string | null>(null)
/** Shared request prevents duplicate metadata discovery across consumers. */
let loadPromise: Promise<ServiceMetadata[]> | null = null
let hasLoaded = false

/** Map of service metadata indexed by service id for quick lookup. */
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
  function load(): Promise<ServiceMetadata[]> {
    if (loadPromise) return loadPromise
    if (hasLoaded) return Promise.resolve(services.value)
    loadPromise = fetchMetadata().finally(() => { loadPromise = null })
    return loadPromise
  }

  /** Fetches display metadata without loading provider homes. */
  async function fetchMetadata(): Promise<ServiceMetadata[]> {
    isLoading.value = true
    errorMessage.value = null

    try {
      const loadedServices = await loadServiceMetadata()
      services.value = loadedServices
      hasLoaded = true
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

  /** Refreshes capabilities and availability using the shared metadata request. */
  function refresh(): Promise<ServiceMetadata[]> {
    hasLoaded = false
    return load()
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
    refresh,
    getService,
  }
}
