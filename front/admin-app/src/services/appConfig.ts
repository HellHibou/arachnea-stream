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

/** Metadata filter definitions configured independently for each service group. */
export interface ServiceFilterDefinition {
  key: string
  /** Translation key resolved through the existing locales dictionaries. */
  label: string
  /** Optional metadata-value to locale-key mapping; values without a mapping stay unchanged. */
  value_labels?: Record<string, string>
}

interface AppConfigState {
  title: string
  serviceStores: string[]
  serviceFilters: Record<string, ServiceFilterDefinition[]>
}

/** Validated configuration value. */
interface ValidatedAppConfig {
  title: string
  serviceStores: string[]
  serviceFilters: Record<string, ServiceFilterDefinition[]>
}

const state = ref<AppConfigState>({ title: '', serviceStores: [], serviceFilters: {} })
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
    serviceFilters: computed(() => state.value.serviceFilters),
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

  const serviceFilters: Record<string, ServiceFilterDefinition[]> = {}
  const rawFilters = payload['service-filters']
  if (rawFilters !== undefined) {
    if (!isRecord(rawFilters)) throw new Error('service-filters must be an object.')
    for (const [store, definitions] of Object.entries(rawFilters)) {
      if (!Array.isArray(definitions)) throw new Error(`Invalid filters for ${store}.`)
      const keys = new Set<string>()
      serviceFilters[store] = definitions.map((definition: unknown) => {
        if (!isRecord(definition) || typeof definition.key !== 'string' || !definition.key.trim()
          || keys.has(definition.key)) throw new Error(`Invalid or duplicate filter for ${store}.`)
        keys.add(definition.key)
        const label = definition.label
        if (typeof label !== 'string' || !label.trim()) {
          throw new Error(`Filter label for ${store}/${definition.key} must be a non-empty translation key.`)
        }
        const valueLabels: Record<string, string> = {}
        if (definition.value_labels !== undefined) {
          if (!isRecord(definition.value_labels)) throw new Error(`Invalid value_labels for ${store}/${definition.key}.`)
          for (const [value, translationKey] of Object.entries(definition.value_labels)) {
            if (typeof translationKey !== 'string' || !translationKey.trim()) {
              throw new Error(`Value label for ${store}/${definition.key}/${value} must be a non-empty translation key.`)
            }
            valueLabels[value] = translationKey.trim()
          }
        }
        return { key: definition.key, label: label.trim(), value_labels: valueLabels }
      })
    }
  }
  return { title: rawTitle.trim(), serviceStores, serviceFilters }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}