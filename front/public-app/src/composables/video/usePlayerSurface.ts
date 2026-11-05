import { computed, ref, shallowRef, watch } from 'vue'
import type { useAttrs } from 'vue'
import type { ResolvedPlayerMediaSource } from '@/services/players'
import { isUrlAllowlisted } from '@/services/videoSourceAllowlist'
import { t } from '@/i18n'
import type { ActiveSurfaceMode, EntryDetailsSurfaceMode, VideoPlayerProps } from './playerTypes'

/**
 * Owns retained sources and transition intent, and derives the shared surface mode.
 * @param props Reactive player props.
 * @param attrs Attributes forwarded to standalone renderers.
 * @returns Public surface refs and narrow transition dependencies for the other slices.
 */
export function usePlayerSurface(props: VideoPlayerProps, attrs: ReturnType<typeof useAttrs>) {
  /** The last resolved media source, used to keep the media surface mounted during transitions. */
  const lastResolvedMediaSource = ref<ResolvedPlayerMediaSource | null>(null)

  /** Whether a language or player selection is waiting for its next source to resolve. */
  const isPlayerSelectionChangePending = shallowRef(false)

  /** Source URL whose preceding playback state should survive a player selection change. */
  const sourcePreservingPlayback = shallowRef<string | null>(null)

  watch(
    () => props.mediaSource,
    (nextMediaSource) => {
      if (nextMediaSource) {
        lastResolvedMediaSource.value = nextMediaSource

        if (isPlayerSelectionChangePending.value) {
          sourcePreservingPlayback.value = nextMediaSource.src
        }

        isPlayerSelectionChangePending.value = false
      }
    },
    { immediate: true },
  )

  /**
   * Indicates whether the component should render the entry details player layout.
   *
   * @returns True when any entry details player feature is active.
   */
  const isEntryDetailsMode = computed(() =>
    props.displayTitle !== null ||
    props.showTrailerPlayer ||
    props.showMediaPlayer ||
    props.mediaOpenUrl !== null ||
    props.showPlayerControls ||
    props.availableLanguages.length > 0 ||
    props.filteredPlayers.length > 0,
  )

  /**
   * Exposes a stable title used by accessibility labels in the entry details player mode.
   *
   * @returns The display title or a default translation.
   */
  const detailTitle = computed(() => props.displayTitle ?? t('player.defaultDisplayTitle'))

  /**
   * Indicates whether the entry details selector bar should be rendered.
   *
   * @returns True when player controls or media open URL is available.
   */
  const showDetailsPlayerControls = computed(() =>
    props.showPlayerControls || Boolean(props.mediaOpenUrl),
  )

  /**
   * Keeps the media surface mounted while an episode, language, or player transition resolves its
   * next media source.
   *
   * @returns True when a persisted media surface should be kept during transition.
   */
  const shouldKeepMediaSurfaceMountedDuringTransition = computed(() =>
    (props.preferPersistedMediaSurface || isPlayerSelectionChangePending.value) &&
    !props.mediaPlayerErrorMessage &&
    Boolean(lastResolvedMediaSource.value) &&
    (
      props.isMediaPlayerLoading ||
      !props.mediaSource
    ),
  )

  /**
   * Keeps the latest playable media source mounted while the next source or page state settles.
   *
   * @returns The active media source or null.
   */
  const renderedMediaSource = computed<ResolvedPlayerMediaSource | null>(() => {
    if (props.mediaSource !== null && props.mediaSource !== undefined) {
      return props.mediaSource
    }
    if (shouldKeepMediaSurfaceMountedDuringTransition.value) {
      const value = lastResolvedMediaSource.value
      if (value !== null && value !== undefined) {
        return value
      }
    }
    return null
  })

  /**
   * Prevents transient page updates from unmounting the media surface while it is still active.
   *
   * @returns True when the media surface should be rendered.
   */
  const shouldRenderMediaSurface = computed(() =>
    props.showMediaPlayer || shouldKeepMediaSurfaceMountedDuringTransition.value,
  )

  /**
   * Prevents the trailer fallback from replacing the media surface during one episode transition.
   *
   * @returns True when the trailer surface should be rendered.
   */
  const shouldRenderTrailerSurface = computed(() =>
    props.showTrailerPlayer && !shouldKeepMediaSurfaceMountedDuringTransition.value,
  )

  /**
   * Indicates which entry-details surface currently owns the player area.
   *
   * @returns The current surface mode: 'media', 'trailer', or null.
   */
  const entryDetailsSurfaceMode = computed<EntryDetailsSurfaceMode | null>(() => {
    if (shouldRenderMediaSurface.value) {
      return 'media'
    }

    if (shouldRenderTrailerSurface.value) {
      return 'trailer'
    }

    return null
  })

  /**
   * Exposes the current render mode for the unified surface template.
   *
   * @returns The active surface mode: 'media', 'trailer', 'standalone', or null.
   */
  const activeSurfaceMode = computed<ActiveSurfaceMode | null>(() => {
    if (isEntryDetailsMode.value) {
      return entryDetailsSurfaceMode.value
    }

    return props.source ? 'standalone' : null
  })

  /**
   * Exposes the one source currently rendered by the unified surface template.
   *
   * @returns The resolved player media source for the active surface.
   */
  const activeSurfaceSource = computed<ResolvedPlayerMediaSource | null>(() => {
    if (!isEntryDetailsMode.value) {
      return props.source ?? null
    }

    if (entryDetailsSurfaceMode.value === 'media') {
      return renderedMediaSource.value
    }

    if (entryDetailsSurfaceMode.value === 'trailer') {
      return props.mediaSource ?? null
    }

    return null
  })

  /**
   * Whether the active surface source URL is covered by the allowlist.
   *
   * The allowlist decides whether an embedded player or trailer may render
   * without going through the security-mode confirmation flow.
   *
   * @returns True when the active source URL matches a whitelisted entry.
   */
  const isActiveSourceAllowlisted = computed(() =>
    isUrlAllowlisted(activeSurfaceSource.value?.src ?? null),
  )

  /**
   * Indicates whether the shared surface wrapper should be rendered.
   *
   * A blocked trailer does not expose an empty player area in `safe` mode.
   * Other blocked embeds retain their dedicated security placeholder.
   *
   * @returns True when a surface should be rendered.
   */
  const shouldRenderSurface = computed(() =>
    isEntryDetailsMode.value
      ? entryDetailsSurfaceMode.value !== null &&
      !(
        entryDetailsSurfaceMode.value === 'trailer' &&
        activeSurfaceSource.value?.renderer === 'iframe' &&
        props.securityMode === 'safe' &&
        !isActiveSourceAllowlisted.value
      )
      : Boolean(props.source),
  )

  /**
   * Indicates whether the current entry-details surface should display the loading overlay.
   *
   * @returns True when loading state should be shown.
   */
  const shouldRenderSurfaceLoadingState = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && props.isMediaPlayerLoading,
  )

  /**
   * Indicates whether the current entry-details surface should display the error state.
   *
   * @returns True when error state should be shown.
   */
  const shouldRenderSurfaceErrorState = computed(() =>
    entryDetailsSurfaceMode.value === 'media' &&
    Boolean(props.mediaPlayerErrorMessage) &&
    !activeSurfaceSource.value,
  )

  /**
   * Exposes the classes applied to the shared surface wrapper.
   *
   * @returns CSS class for the surface container.
   */
  const surfaceContainerClass = computed(() =>
    isEntryDetailsMode.value
      ? 'entry-details__trailer'
      : 'video-player__surface--standalone',
  )

  /**
   * Exposes the classes applied to the loading state overlay when media stays mounted underneath.
   *
   * @returns Object with CSS class modifiers.
   */
  const surfaceStateClasses = computed(() => ({
    'entry-details__player-state--overlay':
      shouldRenderSurfaceLoadingState.value && Boolean(activeSurfaceSource.value),
  }))

  /**
   * Ensures the renderer remounts only when the displayed surface context actually changes.
   *
   * @returns A key string identifying the current surface renderer.
   */
  const activeSurfaceRendererKey = computed(() =>
    activeSurfaceMode.value && activeSurfaceSource.value
      ? `${activeSurfaceMode.value}:${activeSurfaceSource.value.renderer}`
      : 'surface:none',
  )

  /**
   * Exposes the optional attrs forwarded only by standalone renderers.
   *
   * @returns The attrs object or undefined for entry details mode.
   */
  const standaloneRendererAttrs = computed(() =>
    isEntryDetailsMode.value ? undefined : attrs,
  )

  /**
   * Indicates whether the entry-details picker should stay visible below the active media surface.
   *
   * @returns True when the picker should be shown.
   */
  const shouldShowDetailsPlayerPicker = computed(() =>
    entryDetailsSurfaceMode.value === 'media' && showDetailsPlayerControls.value,
  )

  /**
   * Indicates whether the active Video.js renderer should forward progress and lifecycle events.
   *
   * @returns True when events should be forwarded.
   */
  const shouldForwardPrimaryVideoEvents = computed(() =>
    !isEntryDetailsMode.value || entryDetailsSurfaceMode.value === 'media',
  )

  /** Marks a language/player change until its replacement media source arrives. */
  function markSelectionPending() {
    isPlayerSelectionChangePending.value = true
  }

  return {
    state: {
      isEntryDetailsMode,
      detailTitle,
      showDetailsPlayerControls,
      shouldKeepMediaSurfaceMountedDuringTransition,
      renderedMediaSource,
      shouldRenderMediaSurface,
      shouldRenderTrailerSurface,
      entryDetailsSurfaceMode,
      activeSurfaceMode,
      activeSurfaceSource,
      shouldRenderSurface,
      shouldRenderSurfaceLoadingState,
      shouldRenderSurfaceErrorState,
      surfaceContainerClass,
      surfaceStateClasses,
      activeSurfaceRendererKey,
      standaloneRendererAttrs,
      shouldShowDetailsPlayerPicker,
      shouldForwardPrimaryVideoEvents,
    },
    isActiveSourceAllowlisted,
    sourcePreservingPlayback,
    markSelectionPending,
  }
}

export type PlayerSurfaceState = ReturnType<typeof usePlayerSurface>['state']
