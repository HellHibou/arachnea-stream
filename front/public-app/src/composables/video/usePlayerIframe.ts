import { computed, ref, watch, type ComputedRef } from 'vue'
import type { ResolvedIframeMediaSource } from '@/services/players'
import { t } from '@/i18n'
import type { MediaIframeReferrerPolicy, VideoPlayerProps } from './playerTypes'
import type { PlayerSurfaceState } from './usePlayerSurface'

interface PlayerIframeOptions {
  surface: Pick<PlayerSurfaceState, 'isEntryDetailsMode' | 'entryDetailsSurfaceMode' | 'activeSurfaceSource' | 'detailTitle'>
  isActiveSourceAllowlisted: ComputedRef<boolean>
}

/** Default allow attribute for iframes in entry details mode. */
const entryDetailsIframeAllow =
  'accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share'

/**
 * Owns iframe confirmation and derives embed security and renderer attributes.
 * @param props Reactive player props.
 * @param options Current surface refs and the shared allowlist decision.
 * @returns Iframe state, attributes and confirmation action.
 */
export function usePlayerIframe(props: VideoPlayerProps, options: PlayerIframeOptions) {
  const { isEntryDetailsMode, entryDetailsSurfaceMode, activeSurfaceSource, detailTitle } = options.surface
  const { isActiveSourceAllowlisted } = options

  /**
   * Exposes the iframe variant of the current surface source when selected.
   *
   * @returns The iframe media source or null.
   */
  const activeIframeSource = computed<ResolvedIframeMediaSource | null>(() =>
    activeSurfaceSource.value?.renderer === 'iframe'
      ? activeSurfaceSource.value
      : null,
  )

  /**
   * Whether the user accepted loading the embedded iframe in confirmation mode.
   */
  const isIframeConfirmationAccepted = ref(false)

  watch(
    activeIframeSource,
    (nextSource) => {
      if (nextSource) {
        isIframeConfirmationAccepted.value = false
      }
    },
  )

  /**
   * Indicates whether the embedded iframe should be rendered immediately.
   *
   * Standalone surfaces always render iframes; restrictions apply only to
   * entry details mode. In `unsafe` mode every embed renders, in `confirmation`
   * mode an allowlisted or accepted source renders, and in `safe` mode only
   * allowlisted sources render.
   *
   * @returns True when the active iframe may be rendered.
   */
  const shouldRenderIframe = computed(() =>
    !isEntryDetailsMode.value ||
    props.securityMode === 'unsafe' ||
    (props.securityMode === 'confirmation' &&
      (isIframeConfirmationAccepted.value || isActiveSourceAllowlisted.value)) ||
    (props.securityMode === 'safe' && isActiveSourceAllowlisted.value),
  )

  /**
   * Indicates whether the embedded iframe should be shown as a confirmation placeholder.
   *
   * @returns True when the entry details mode is 'confirmation', the confirmation
   * has not been accepted, and the source is not allowed by the allowlist.
   */
  const shouldShowIframeConfirmation = computed(() =>
    isEntryDetailsMode.value &&
    Boolean(activeIframeSource.value) &&
    props.securityMode === 'confirmation' &&
    !isIframeConfirmationAccepted.value &&
    !isActiveSourceAllowlisted.value,
  )

  /**
   * Indicates whether the embedded iframe should be blocked entirely.
   *
   * In `safe` mode a blocked embed renders a placeholder only when its source is
   * not allowlisted; allowlisted sources stay playable.
   *
   * @returns True when the entry details mode is 'safe' and the iframe source is not allowlisted.
   */
  const shouldBlockIframe = computed(() =>
    isEntryDetailsMode.value &&
    Boolean(activeIframeSource.value) &&
    props.securityMode === 'safe' &&
    !isActiveSourceAllowlisted.value,
  )

  /**
   * Indicates whether the source video action should be hidden.
   *
   * @returns True when the security mode is 'safe'.
   */
  const shouldHideSourceVideoAction = computed(() => props.securityMode === 'safe')

  /**
   * Handles the iframe confirmation acceptance by the user.
   */
  function handleIframeConfirmationAccept() {
    isIframeConfirmationAccepted.value = true
  }

  /**
   * Exposes the title applied to the unified iframe renderer.
   *
   * @returns The appropriate title based on mode and surface.
   */
  const activeIframeTitle = computed(() => {
    if (!isEntryDetailsMode.value) {
      return props.iframeTitle
    }

    if (entryDetailsSurfaceMode.value === 'trailer') {
      return t('player.trailerTitle', { title: detailTitle.value })
    }

    if (entryDetailsSurfaceMode.value === 'media') {
      return t('media.videoTitle', { title: detailTitle.value })
    }

    return null
  })

  /**
   * Exposes the permissions string applied to the unified iframe renderer.
   *
   * @returns The allow attribute value.
   */
  const activeIframeAllow = computed(() =>
    isEntryDetailsMode.value ? entryDetailsIframeAllow : props.allow,
  )

  /**
   * Indicates whether the unified iframe renderer should allow fullscreen.
   *
   * @returns True when fullscreen should be allowed.
   */
  const activeIframeAllowFullscreen = computed(() =>
    isEntryDetailsMode.value || props.allowFullscreen,
  )

  /**
   * Exposes the loading strategy applied to the unified iframe renderer.
   *
   * @returns The loading strategy: 'lazy' or 'eager'.
   */
  const activeIframeLoading = computed(() =>
    isEntryDetailsMode.value ? 'lazy' : props.loading,
  )

  /**
   * Exposes the referrer policy applied to the unified iframe renderer.
   *
   * @returns The referrer policy or null.
   */
  const activeIframeReferrerPolicy = computed<MediaIframeReferrerPolicy | null>(() => {
    const result = isEntryDetailsMode.value ? 'strict-origin-when-cross-origin' : props.referrerPolicy
    return result ?? null
  })

  /**
   * Exposes the accessibility attributes applied to the unified iframe renderer.
   *
   * @returns Whether the iframe should be hidden from accessibility APIs.
   */
  const activeIframeAriaHidden = computed(() =>
    isEntryDetailsMode.value ? false : props.ariaHidden,
  )

  /**
   * Exposes the tab index applied to the unified iframe renderer.
   *
   * @returns The tab index or null for entry details mode.
   */
  const activeIframeTabIndex = computed(() =>
    isEntryDetailsMode.value ? null : props.tabIndex,
  )

  /**
   * Exposes the classes applied to the unified iframe renderer.
   *
   * @returns CSS class for the iframe.
   */
  const activeIframeClass = computed(() =>
    isEntryDetailsMode.value ? 'entry-details__trailer-frame' : 'video-player__iframe--standalone',
  )

  return {
    shouldShowIframeConfirmation,
    shouldBlockIframe,
    shouldHideSourceVideoAction,
    shouldRenderIframe,
    isIframeConfirmationAccepted,
    activeIframeSource,
    activeIframeTitle,
    activeIframeAllow,
    activeIframeAllowFullscreen,
    activeIframeLoading,
    activeIframeReferrerPolicy,
    activeIframeAriaHidden,
    activeIframeTabIndex,
    activeIframeClass,
    handleIframeConfirmationAccept,
  }
}
