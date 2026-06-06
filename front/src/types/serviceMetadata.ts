/**
 * Source-specific theme mapping advertised by a backend service.
 */
export interface ServiceThemeMetadata {
  code: string
  serviceCode: string
}

/**
 * Display metadata exposed by one backend service.
 */
export interface ServiceMetadata {
  id: string
  title: string
  logo: string | null
  description: Record<string, string>
  themes: ServiceThemeMetadata[]
}
