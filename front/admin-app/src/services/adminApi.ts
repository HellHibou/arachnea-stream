import { getAdminApiUrl } from '@/services/baseUrl'

/**
 * Stable error shape returned by the admin API.
 * Every error response carries `{error:{code,message}}`.
 */
export interface AdminApiError {
  code: string
  message: string
}

/**
 * Parsed error envelope from the backend.
 */
interface ErrorEnvelope {
  error?: AdminApiError
}

/**
 * Custom error class for admin API failures.
 */
export class AdminApiException extends Error {
  /** Stable machine-readable error code. */
  public readonly code: string
  /** HTTP status code when available. */
  public readonly status: number

  constructor(code: string, message: string, status: number = 0) {
    super(message)
    this.name = 'AdminApiException'
    this.code = code
    this.status = status
  }
}

/**
 * Options for an admin API request.
 */
interface ApiRequestOptions {
  /** HTTP method. Defaults to GET. */
  method?: 'GET' | 'POST' | 'PUT' | 'DELETE'
  /** Optional JSON body for POST/PUT requests. */
  body?: unknown
  /** Optional query parameters. */
  params?: Record<string, string | number | boolean | undefined>
}

interface TauriInvoke {
  (command: string, args?: Record<string, unknown>): Promise<unknown>
}

interface TauriWindow extends Window {
  __TAURI__?: {
    core?: { invoke?: TauriInvoke }
    invoke?: TauriInvoke
  }
}

/**
 * Detects the Tauri invoke bridge when the app runs inside a desktop window.
 *
 * @returns The Tauri invoke function, or `undefined` in a plain browser.
 */
function getTauriInvoke(): TauriInvoke | undefined {
  const tauriWindow = window as TauriWindow
  return tauriWindow.__TAURI__?.core?.invoke ?? tauriWindow.__TAURI__?.invoke
}
/**
 * Performs a typed admin API call with normalized error handling.
 *
 * In a plain browser the request goes through `fetch`; inside a Tauri desktop
 * window the same operation is invoked through the `admin/<operation>` IPC
 * command exposed by the backend, which shares the exact response contract.
 *
 * @param operation - Admin operation name.
 * @param options - Request options.
 * @returns Parsed JSON response.
 * @throws AdminApiException on failure.
 */
async function apiCall<T>(operation: string, options: ApiRequestOptions = {}): Promise<T> {
  const { method = 'GET', body, params } = options

  const tauriInvoke = getTauriInvoke()
  if (tauriInvoke) {
    const payload: Record<string, unknown> = {}
    if (params) {
      for (const [key, value] of Object.entries(params)) {
        if (value !== undefined) {
          payload[key] = value
        }
      }
    }
    if (body && method !== 'GET' && typeof body === 'object' && body !== null) {
      Object.assign(payload, body)
    }
    try {
      const data = await tauriInvoke(`admin/${operation}`, payload)
      const envelope = data as ErrorEnvelope | undefined
      if (envelope?.error) {
        throw new AdminApiException(envelope.error.code, envelope.error.message, 0)
      }
      return data as T
    } catch (error) {
      if (error instanceof AdminApiException) {
        throw error
      }
      const message = error instanceof Error ? error.message : String(error)
      throw new AdminApiException('request_failed', message, 0)
    }
  }
  let url = getAdminApiUrl(operation)
  if (params) {
    const searchParams = new URLSearchParams()
    for (const [key, value] of Object.entries(params)) {
      if (value !== undefined) {
        searchParams.set(key, String(value))
      }
    }
    const queryString = searchParams.toString()
    if (queryString) {
      url += `?${queryString}`
    }
  }

  const fetchOptions: RequestInit = {
    method,
    headers: {
      'Accept': 'application/json',
    },
    credentials: 'include',
  }

  if (body && method !== 'GET') {
    fetchOptions.headers = {
      ...fetchOptions.headers,
      'Content-Type': 'application/json',
    }
    fetchOptions.body = JSON.stringify(body)
  }

  const response = await fetch(url, fetchOptions)

  // Handle empty responses (204, logout, etc.)
  if (response.status === 204) {
    return undefined as T
  }

  const text = await response.text()
  let data: unknown = undefined
  if (text) {
    try {
      data = JSON.parse(text)
    } catch {
      throw new AdminApiException(
        'invalid_response',
        'Invalid JSON response from server.',
        response.status,
      )
    }
  }

  if (!response.ok) {
    const envelope = data as ErrorEnvelope | undefined
    const error = envelope?.error
    throw new AdminApiException(
      error?.code ?? 'request_failed',
      error?.message ?? `Request failed with status ${response.status}.`,
      response.status,
    )
  }

  return data as T
}

// ─── DTOs (mirroring backend admin/dto.rs) ─────────────────────────────────

export interface CredentialInfo {
  required: boolean
  signup_url?: string
  configured: boolean
  login_masked?: string
}

export interface AdminServiceEntry {
  id: string
  title?: string
  logo?: string
  description?: string
  default_enabled: boolean
  enabled: boolean
  has_override: boolean
  credentials?: CredentialInfo
  unavailable: boolean
}

export interface AdminUnavailableSource {
  id?: string
  path: string
  message: string
}

export interface ServicesResponse {
  services: AdminServiceEntry[]
  unavailable: AdminUnavailableSource[]
}

export interface StatusCapabilities {
  services: boolean
  credentials: boolean
  settings: boolean
  admin_password: boolean
  reload: boolean
}

export interface StatusResponse {
  mode: 'desktop' | 'server'
  auth_required: boolean
  authenticated: boolean
  password_configured: boolean
  capabilities: StatusCapabilities
}

export interface LoginRequest {
  password: string
}

export interface LoginResponse {
  authenticated: boolean
}

export interface ServiceEnabledResponse {
  service_id: string
  enabled?: boolean
  reload_required: boolean
}

export interface SetCredentialsRequest {
  service_id: string
  login: string
  password: string
}

export interface ClearCredentialsRequest {
  service_id: string
}

export interface CredentialsResponse {
  credentials: Record<string, CredentialInfo>
}

export type SettingSource = 'command_line' | 'configuration' | 'default'

export interface SettingsResponse {
  server_port: number
  server_port_source: SettingSource
  network_mode: string
  network_mode_source: SettingSource
  entrypoint_root?: string
  entrypoint_root_source: SettingSource
  public_http_warning: boolean
}

export interface UpdateSettingsRequest {
  server_port?: number
  network_mode?: 'local' | 'private' | 'public'
  entrypoint_root?: string
}

export interface UpdateSettingsResponse {
  /** Kept for backward compatibility: true only when hot apply is unavailable. */
  restart_required: boolean
  /** Whether hot application of the settings has been scheduled on the running server. */
  applied: boolean
  /** Scheduling failure context, when hot application could not be planned. */
  apply_error?: string | null
  /** Target admin URL when the port or the entrypoint root change. */
  admin_url?: string | null
}

export interface SetAdminPasswordRequest {
  current_password?: string
  new_password: string
}

export interface AdminReloadSkippedSource {
  id?: string
  path: string
}

export interface AdminReloadSourceError {
  id?: string
  path: string
  message: string
}

export interface ReloadResponse {
  applied: boolean
  loaded: string[]
  disabled: string[]
  ignored: AdminReloadSkippedSource[]
  errors: AdminReloadSourceError[]
  build_error?: string
}

// ─── API Operations ────────────────────────────────────────────────────────

/** Fetches the admin status: mode, capabilities, and auth state. */
export function fetchStatus(): Promise<StatusResponse> {
  return apiCall<StatusResponse>('status')
}

/** Attempts to log in with the given password. */
export function login(password: string): Promise<LoginResponse> {
  return apiCall<LoginResponse>('login', {
    method: 'POST',
    body: { password } satisfies LoginRequest,
  })
}

/** Logs out the current session. */
export function logout(): Promise<void> {
  return apiCall<void>('logout', { method: 'POST' })
}

/** Fetches the service catalog in the requested language. */
export function fetchServices(lang?: string): Promise<ServicesResponse> {
  return apiCall<ServicesResponse>('services', {
    params: { lang },
  })
}

/** Sets the enabled override for a service. */
export function setServiceEnabled(serviceId: string, enabled: boolean): Promise<ServiceEnabledResponse> {
  return apiCall<ServiceEnabledResponse>('set-service-enabled', {
    method: 'POST',
    body: { service_id: serviceId, enabled },
  })
}

/** Resets the enabled override for a service. */
export function resetServiceEnabled(serviceId: string): Promise<ServiceEnabledResponse> {
  return apiCall<ServiceEnabledResponse>('reset-service-enabled', {
    method: 'POST',
    body: { service_id: serviceId },
  })
}

/** Fetches credential information for all services or a specific one. */
export function fetchCredentials(serviceId?: string): Promise<CredentialsResponse> {
  return apiCall<CredentialsResponse>('credentials', {
    params: serviceId ? { serviceId } : undefined,
  })
}

/** Sets credentials for a service. */
export function setCredentials(serviceId: string, login: string, password: string): Promise<void> {
  return apiCall<void>('set-credentials', {
    method: 'POST',
    body: { service_id: serviceId, login, password } satisfies SetCredentialsRequest,
  })
}

/** Clears stored credentials for a service. */
export function clearCredentials(serviceId: string): Promise<void> {
  return apiCall<void>('clear-credentials', {
    method: 'POST',
    body: { service_id: serviceId } satisfies ClearCredentialsRequest,
  })
}

/** Fetches current effective settings with their provenance. */
export function fetchSettings(): Promise<SettingsResponse> {
  return apiCall<SettingsResponse>('settings')
}

/** Updates persisted settings and schedules their hot application. */
export function updateSettings(settings: UpdateSettingsRequest): Promise<UpdateSettingsResponse> {
  return apiCall<UpdateSettingsResponse>('update-settings', {
    method: 'POST',
    body: settings,
  })
}

/** Changes the administrator password. */
export function setAdminPassword(currentPassword: string | undefined, newPassword: string): Promise<void> {
  return apiCall<void>('set-admin-password', {
    method: 'POST',
    body: {
      current_password: currentPassword,
      new_password: newPassword,
    } satisfies SetAdminPasswordRequest,
  })
}

/** Triggers a configuration reload. */
export function reload(): Promise<ReloadResponse> {
  return apiCall<ReloadResponse>('reload', { method: 'POST', body: {} })
}
