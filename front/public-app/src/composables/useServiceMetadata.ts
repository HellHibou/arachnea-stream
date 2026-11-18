import { computed, readonly, shallowRef } from 'vue'

import { t } from '@/i18n'
import { sessionGeneration } from '@/services/sessionEvents'
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
window.addEventListener('arachnea-session-context', () => { services.value = []; hasLoaded = false; loadPromise = null; errorMessage.value = null })

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
    const pending = fetchMetadata().finally(() => { if (loadPromise === pending) loadPromise = null })
    loadPromise = pending
    return pending
  }

  /** Fetches display metadata without loading provider homes. */
  async function fetchMetadata(): Promise<ServiceMetadata[]> {
    isLoading.value = true
    errorMessage.value = null

    const generation = sessionGeneration()
    try {
      const loadedServices = await loadServiceMetadata()
      if (generation !== sessionGeneration()) return []
      services.value = loadedServices
      hasLoaded = true
      return loadedServices
    } catch (error) {
      if (generation !== sessionGeneration()) return []
      errorMessage.value =
        error instanceof Error
          ? error.message
          : t('errors.serviceMetadata')
      return []
    } finally {
      if (generation === sessionGeneration()) isLoading.value = false
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
