/** Origin that produced a structured scraper execution error. */
export type ScraperErrorOrigin = 'backend' | 'frontend'

/** One backend source failure or a frontend transport/protocol failure. */
export interface ScraperExecutionError {
  /** Correlation code used to locate matching backend logs or browser console output. */
  code: string
  /** Backend command or frontend operation that failed. */
  operation: string
  /** Scraper source when known, or null for pre-execution and transport failures. */
  source: string | null
  /** Runtime side that produced the error. */
  origin: ScraperErrorOrigin
  /** Technical diagnostic detail, not intended as the default UI message. */
  message: string
}

/** Stable JSON envelope returned by StreamScraper commands. */
export interface ScraperAggregationResult<T> {
  /** Command-specific successful payload. */
  data: T
  /** Source-scoped or technical execution failures. */
  errors: ScraperExecutionError[]
}
