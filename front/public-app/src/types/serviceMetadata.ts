/**
 * Source-specific theme mapping advertised by a backend service.
 */
export interface ServiceThemeMetadata {
  /** The unique identifier for the theme. */
  code: string
  /** The code of the service this theme belongs to. */
  serviceCode: string
}

/**
 * Display metadata exposed by one backend service.
 */
export interface ServiceMetadata {
  /** The unique identifier for the service. */
  id: string
  /** The display title of the service. */
  title: string
  /** The URL or path to the service logo, or null if no logo. */
  logo: string | null
  /** Localized descriptions keyed by language code. */
  description: Record<string, string>
  /** The list of themes associated with this service. */
  themes: ServiceThemeMetadata[]
}
