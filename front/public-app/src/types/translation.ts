/** Supported source subtitle formats for the initial translation transport. */
export type SubtitleTranslationFormat = 'webvtt'

/** One directional source/target language pair declared by a service. */
export interface TranslationLanguagePair {
  /** Source language code. */
  source: string
  /** Target language code. */
  target: string
}

/** Public capabilities of one enabled translation service. */
export interface TranslationServiceCapabilities {
  /** Stable service identifier. */
  serviceId: string
  /** Human-readable service title. */
  title: string
  /** Languages declared by the service, usable in both directions by default. */
  languages: string[]
  /** Effective source languages. */
  sourceLanguages: string[]
  /** Effective target languages. */
  targetLanguages: string[]
  /** Restricted directional pairs, or `null` when every declared pair is usable. */
  supportedPairs: TranslationLanguagePair[] | null
  /** Whether the service can translate text blocks. */
  supportsTextTranslation: boolean
  /** Whether the service can translate a document from its URL. */
  supportsUrlTranslation: boolean
  /** Whether the service can translate without an explicit source language. */
  sourceDetection: boolean
  /** Whether the service is ranked first in automatic mode. */
  preferred: boolean
}

/** Public translation catalog returned by `get_translation_services`. */
export interface TranslationCatalog {
  /** Enabled services with effective capabilities. */
  services: TranslationServiceCapabilities[]
  /** Union of languages reachable through the enabled services. */
  languages: string[]
}

/** Automatic selection, optionally preferring one compatible service. */
export interface AutomaticSubtitleTranslationPolicy {
  /** Discriminator for automatic service selection. */
  mode: 'auto'
  /** Optional preferred service identifier used only as a ranking hint. */
  preferredService?: string | null
}

/** Selection restricted to one explicitly requested service. */
export interface ConstrainedSubtitleTranslationPolicy {
  /** Discriminator for constrained service selection. */
  mode: 'constrained'
  /** Required service identifier. */
  serviceId: string
}

/** Selection policy accepted by the backend preparation command. */
export type SubtitleTranslationPolicy =
  | AutomaticSubtitleTranslationPolicy
  | ConstrainedSubtitleTranslationPolicy

/** Source subtitle identity and location used to prepare a translation. */
export interface SubtitleTranslationSource {
  /** Direct or application-proxy subtitle URL. */
  url: string
  /** Stable source track identifier when already known. */
  trackId?: string | null
  /** Source subtitle format. Only WebVTT is accepted initially. */
  format?: SubtitleTranslationFormat
}

/**
 * Language request produced by the subtitle preference layer.
 *
 * This type intentionally carries no temporary token, URL or translated blob.
 */
export interface SubtitleTranslationLanguageRequest {
  /** Authorized source track. */
  source: SubtitleTranslationSource
  /** Requested target language. */
  targetLanguage: string
  /** Known source language, or omitted for provider-side detection. */
  sourceLanguage?: string | null
  /** Automatic or constrained service policy. */
  policy?: SubtitleTranslationPolicy
  /** Optional display label override for the generated track. */
  label?: string | null
}

/** Track-shaped preparation response compatible with the existing player contract. */
export interface PreparedSubtitleTranslation {
  /** Public tokenized WebVTT file URL. */
  link: string
  /** Effective target language. */
  lang: string
  /** Display label for the generated track. */
  label: string
  /** Always true for a translated track. */
  autoTranslate: true
}

/** Successful file download retained by the browser cache. */
export interface SubtitleTranslationFile {
  /** Complete WebVTT document. */
  blob: Blob
  /** Effective target language. */
  lang: string
  /** Display label for the generated track. */
  label: string
  /** Composite validator returned by the file transport, when provided. */
  etag: string | null
}
