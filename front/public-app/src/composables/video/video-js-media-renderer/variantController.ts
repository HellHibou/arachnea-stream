import { computed, shallowRef, watch } from 'vue'
import type { ResolvedVideoMediaSource, ResolvedVideoVariant } from '@/services/players'

/** Format priority belongs to the client; stable ordering breaks ties. */
const FORMAT_PRIORITY = { hls: 0, dash: 1, file: 2 } as const

/**
 * Owns variant selection and bounded fallback for one content collection.
 * @param getSource Reactive reader for the collection and its shared metadata.
 * @param getPreferences Initial language and quality preferences from the player session.
 * @returns Active media source, variant state and explicit selection/failure operations.
 */
export function createVariantController(getSource: () => ResolvedVideoMediaSource, getPreferences: () => { lang: string | null; quality: string | null }) {
  const activeIndex = shallowRef(0)
  const revision = shallowRef(0)
  const failed = new Set<number>()
  let preferredLanguage = getPreferences().lang
  let preferredQuality = getPreferences().quality
  const variants = computed<ResolvedVideoVariant[]>(() => getSource().sources ?? [{
    ...getSource(), quality: null, lang: null,
  }])

  /**
   * Uses available engine capabilities as a prefilter; real playback remains decisive.
   * @param variant Technical alternative to inspect.
   */
  function canUseVariant(variant: ResolvedVideoVariant): boolean {
    if (typeof document === 'undefined') return true
    const video = document.createElement('video')
    const hasMediaSource = 'MediaSource' in window || 'ManagedMediaSource' in window
    if (variant.transport === 'dash') return hasMediaSource
    if (variant.transport === 'hls') return hasMediaSource || Boolean(video.canPlayType(variant.mimeType ?? ''))
    return !variant.mimeType || Boolean(video.canPlayType(variant.mimeType))
  }

  /**
   * Ranks adaptive formats before MP4 and the other supported files.
   * @param variant Format advertised independently of the URL extension.
   */
  function formatRank(variant: ResolvedVideoVariant): number {
    return variant.transport === 'file' && variant.mimeType !== 'video/mp4' ? 3 : FORMAT_PRIORITY[variant.transport]
  }

  /**
   * Orders unfailed candidates using user dimensions before transport priority.
   * @param language Preferred language, or null for the default ordering.
   * @param quality Preferred fixed quality, or null for the default ordering.
   */
  function candidates(language = preferredLanguage, quality = preferredQuality): number[] {
    const languageRank = (lang: string | null) => !language || lang === language ? 0 : lang === null ? 1 : 2
    return variants.value.map((_, index) => index).filter((index) => !failed.has(index) && canUseVariant(variants.value[index]!))
      .sort((a, b) => {
        const left = variants.value[a]!, right = variants.value[b]!
        return languageRank(left.lang) - languageRank(right.lang) ||
          Number(Boolean(quality && left.quality !== quality)) - Number(Boolean(quality && right.quality !== quality)) ||
          formatRank(left) - formatRank(right) || a - b
      })
  }

  watch(getSource, () => {
    failed.clear()
    activeIndex.value = candidates()[0] ?? 0
    revision.value++
  }, { immediate: true, flush: 'sync' })

  const source = computed<ResolvedVideoMediaSource>(() => ({
    ...getSource(), ...variants.value[activeIndex.value],
  }))

  /**
   * Selects a language or quality without losing the user's other preference.
   * @param dimension Menu dimension changed by the user.
   * @param value Desired quality or language.
   */
  function select(dimension: 'lang' | 'quality', value: string | null): void {
    const active = variants.value[activeIndex.value]
    if (dimension === 'lang') preferredLanguage = value
    else preferredQuality = value
    const next = candidates(
      dimension === 'lang' ? value : preferredLanguage ?? active?.lang ?? null,
      dimension === 'quality' ? value : preferredQuality ?? active?.quality ?? null,
    ).find((index) => variants.value[index]?.[dimension] === value)
    if (next !== undefined) activeIndex.value = next
    revision.value++
  }

  /** Marks the active candidate failed and returns whether a fallback was selected. */
  function fail(): boolean {
    const active = variants.value[activeIndex.value]
    failed.add(activeIndex.value)
    const next = candidates(preferredLanguage ?? active?.lang ?? null, preferredQuality ?? active?.quality ?? null)[0]
    revision.value++
    if (next === undefined) return false
    activeIndex.value = next
    return true
  }

  /**
   * Reports whether a particular candidate remains usable in this generation.
   * @param index Candidate index in the current collection.
   */
  function isAvailable(index: number): boolean {
    return !failed.has(index) && Boolean(variants.value[index] && canUseVariant(variants.value[index]!))
  }

  /** Returns user preferences independently of the temporary fallback candidate. */
  function preferences() {
    return { lang: preferredLanguage, quality: preferredQuality }
  }

  return { source, variants, activeIndex, revision, select, fail, isAvailable, preferences }
}

export type VariantController = ReturnType<typeof createVariantController>
