import { ref } from 'vue'

import {
  fetchStatus,
  login as apiLogin,
  logout as apiLogout,
  fetchServices,
  setServiceEnabled as apiSetServiceEnabled,
  resetServiceEnabled as apiResetServiceEnabled,
  fetchCredentials,
  setCredentials as apiSetCredentials,
  clearCredentials as apiClearCredentials,
  fetchSettings,
  updateSettings as apiUpdateSettings,
  setAdminPassword as apiSetAdminPassword,
  reload as apiReload,
  type StatusResponse,
  type ServicesResponse,
  type CredentialsResponse,
  type SettingsResponse,
  type ReloadResponse,
  type ServiceEnabledResponse,
  type UpdateSettingsResponse,
  type UpdateSettingsRequest,
  AdminApiException,
} from '@/services/adminApi'
import { useI18n } from '@/i18n'
import { useErrorNotifications } from '@/composables/useErrorNotifications'

/**
 * Network error state for the admin API.
 */
export interface ApiError {
  code: string
  message: string
  /** HTTP status code when the error came from an HTTP response. */
  status?: number
}

/**
 * Composable providing reactive admin API operations with error handling.
 */
export function useAdminApi() {
  const { t } = useI18n()
  const { pushError } = useErrorNotifications()
  const isLoading = ref(false)
  const error = ref<ApiError | null>(null)

  /**
   * Clears the current error state.
   */
  function clearError(): void {
    error.value = null
  }

  /**
   * Normalizes any thrown value into an ApiError.
   *
   * @param err - The caught error.
   * @returns Normalized ApiError.
   */
  function normalizeError(err: unknown): ApiError {
    if (err instanceof AdminApiException) {
      return { code: err.code, message: err.message, status: err.status }
    }
    if (err instanceof Error) {
      return { code: 'network_error', message: err.message }
    }
    return { code: 'unknown_error', message: t('error.unknown') }
  }

  /**
   * Wraps an API call with loading and error state management.
   *
   * @param fn - Async function to execute.
   * @returns Result of the function, or undefined on error.
   */
  async function withApiState<T>(fn: () => Promise<T>): Promise<T | undefined> {
    isLoading.value = true
    error.value = null
    try {
      return await fn()
    } catch (err) {
      error.value = normalizeError(err)
      pushError({
        code: error.value.code,
        message: error.value.message,
        status: error.value.status,
      })
      return undefined
    } finally {
      isLoading.value = false
    }
  }

  // ─── Status & Auth ────────────────────────────────────────────────────────

  async function getStatus(): Promise<StatusResponse | undefined> {
    return withApiState(() => fetchStatus())
  }

  async function login(password: string): Promise<boolean> {
    const result = await withApiState(() => apiLogin(password))
    return result?.authenticated ?? false
  }

  async function logout(): Promise<void> {
    await withApiState(() => apiLogout())
  }

  // ─── Services ─────────────────────────────────────────────────────────────

  async function getServices(lang?: string): Promise<ServicesResponse | undefined> {
    return withApiState(() => fetchServices(lang))
  }

  async function setServiceEnabled(
    serviceStoreId: string,
    serviceId: string,
    enabled: boolean,
  ): Promise<ServiceEnabledResponse | undefined> {
    return withApiState(() => apiSetServiceEnabled(serviceStoreId, serviceId, enabled))
  }

  async function resetServiceEnabled(
    serviceStoreId: string,
    serviceId: string,
  ): Promise<ServiceEnabledResponse | undefined> {
    return withApiState(() => apiResetServiceEnabled(serviceStoreId, serviceId))
  }

  // ─── Credentials ──────────────────────────────────────────────────────────

  async function getCredentials(
    serviceStoreId?: string,
    serviceId?: string,
  ): Promise<CredentialsResponse | undefined> {
    return withApiState(() => fetchCredentials(serviceStoreId, serviceId))
  }

  async function setCredentials(
    serviceStoreId: string,
    serviceId: string,
    login: string,
    password: string,
  ): Promise<boolean> {
    return withApiState(() => apiSetCredentials(serviceStoreId, serviceId, login, password)) !== undefined
  }

  async function clearCredentials(serviceStoreId: string, serviceId: string): Promise<boolean> {
    return withApiState(() => apiClearCredentials(serviceStoreId, serviceId)) !== undefined
  }

  // ─── Settings ─────────────────────────────────────────────────────────────

  async function getSettings(): Promise<SettingsResponse | undefined> {
    return withApiState(() => fetchSettings())
  }

  async function updateSettings(settings: UpdateSettingsRequest): Promise<UpdateSettingsResponse | undefined> {
    return withApiState(() => apiUpdateSettings(settings))
  }

  async function setAdminPassword(currentPassword: string | undefined, newPassword: string): Promise<boolean> {
    return withApiState(() => apiSetAdminPassword(currentPassword, newPassword)) !== undefined
  }

  // ─── Reload ───────────────────────────────────────────────────────────────

  async function reload(): Promise<ReloadResponse | undefined> {
    return withApiState(() => apiReload())
  }

  return {
    isLoading,
    error,
    clearError,
    getStatus,
    login,
    logout,
    getServices,
    setServiceEnabled,
    resetServiceEnabled,
    getCredentials,
    setCredentials,
    clearCredentials,
    getSettings,
    updateSettings,
    setAdminPassword,
    reload,
  }
}
