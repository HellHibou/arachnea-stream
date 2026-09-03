import { computed, readonly, ref } from 'vue'

import { resolveAppPath } from '@/services/baseUrl'

/**
 * Validation and access to the public application configuration
 * (`public/config.json`).
 *
 * The published resource uses the `service-store` key (dash spelling, as
 * defined by the contract); TypeScript represents it as `'service-store'` and
 * the code must not invent a second camelCase form.
 */

const CONFIG_PATH = 'config.json'

interface AppConfigState {
  title: string
  serviceStores: string[]
}

/** Validated configuration value. */
interface ValidatedAppConfig {
  title: string
  serviceStores: string[]
}

const state = ref<AppConfigState>({ title: '', serviceStores: [] })
const isLoading = ref(false)
const errorMessage = ref<string | null>(null)
const loaded = ref(false)

/**
 * Loads and validates the public application configuration.
 *
 * Called once during bootstrap, before the app is mounted. A missing or
 * invalid configuration leaves a localized error exposed through
 * {@link useAppConfig} instead of presenting an incomplete administration.
 */
export async function initializeAppConfig(): Promise<void> {
  if (loaded.value || isLoading.value || errorMessage.value) {
    return
  }
  isLoading.value = true
  errorMessage.value = null
  try {
    const response = await fetch(resolveAppPath(CONFIG_PATH))
    if (!response.ok) {
      throw new Error(`Unable to load application configuration (${response.status}).`)
    }
    const payload: unknown = await response.json()
    state.value = validateAppConfig(payload)
    loaded.value = true
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error)
  } finally {
    isLoading.value = false
  }
}

/**
 * Reactive access to the application configuration.
 */
export function useAppConfig() {
  return {
    title: computed(() => state.value.title),
    serviceStores: computed(() => state.value.serviceStores),
    isLoading: readonly(isLoading),
    errorMessage: readonly(errorMessage),
    loaded: readonly(loaded),
  }
}

/**
 * Returns the route path of the first configured service group.
 *
 * Used as the default services destination before any group selection. Falls
 * back to the settings route when no group is configured so navigation never
 * loops on the `/services` redirect.
 */
export function defaultServicesPath(): string {
  const firstStore = state.value.serviceStores[0]
  return firstStore ? `/services/${firstStore}` : '/settings'
}

/**
 * Whether the given technical identifier is a configured service group.
 */
export function hasConfiguredServiceStore(serviceStoreId: string): boolean {
  return state.value.serviceStores.includes(serviceStoreId)
}

function validateAppConfig(payload: unknown): ValidatedAppConfig {
  if (!isRecord(payload)) {
    throw new Error('The application configuration must be a JSON object.')
  }

  const rawTitle = payload.title
  if (typeof rawTitle !== 'string' || rawTitle.trim().length === 0) {
    throw new Error('The application configuration title must be a non-empty string.')
  }

  const rawStores = payload['service-store']
  if (!Array.isArray(rawStores) || rawStores.length === 0) {
    throw new Error('The application configuration service-store must be a non-empty array.')
  }

  const serviceStores: string[] = []
  for (const rawStore of rawStores) {
    if (typeof rawStore !== 'string' || rawStore.trim().length === 0) {
      throw new Error('Every service-store entry must be a non-empty string.')
    }
    const store = rawStore.trim()
    if (serviceStores.includes(store)) {
      throw new Error(`Duplicate service store identifier ${store}.`)
    }
    serviceStores.push(store)
  }

  return { title: rawTitle.trim(), serviceStores }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}