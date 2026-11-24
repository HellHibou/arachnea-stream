<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { useServiceMetadata } from '@/composables/useServiceMetadata'
import { useI18n } from '@/i18n'
import { categoryMediaTarget, mediaRoute } from '@/services/mediaRouting'
import { normalizeHomeCategory } from '@/services/rustify/home/categories'
import type { EntryChannel } from '@/types/entry'

/**
 * Props accepted by the poster panel rendered on the left side of the entry details page.
 */
interface Props {
  /**
   * Stable title used as the poster image alternative text and fallback label.
   */
  displayTitle: string
  /**
   * Image rendered inside the poster frame when available.
   */
  posterFrameImageUrl: string | null
  /**
   * Indicates whether the poster frame should preserve the entire image.
   */
  posterFrameUsesContain: boolean
  /**
   * Indicates whether the trailer toggle button should be displayed.
   */
  showTrailerAction: boolean
  /**
   * Label rendered inside the trailer toggle button.
   */
  trailerActionLabel: string
  /**
   * Public entry URL opened by the primary action.
   */
  entryUrl: string | null
  /**
   * Backend source id used to resolve display metadata.
   */
  source: string | null
  /**
   * Optional channel attribution linking to its category page.
   */
  channel?: EntryChannel | null
  /**
   * Indicates whether the source entry action should be visible.
   * @default true
   */
  showSourceEntryAction?: boolean
}

/** Component props without defaults. */
const props = withDefaults(defineProps<Props>(), {
  channel: null,
  showSourceEntryAction: true,
})
const emit = defineEmits<{
  /** Emitted when the trailer toggle is clicked. */
  'toggle-trailer': []
}>()

/** Whether the source logo is available for display. */
const sourceLogoAvailable = ref(true)
/** Service metadata utilities. */
const { getService } = useServiceMetadata()
/** Internationalization utilities. */
const { t } = useI18n()

/** Metadata for the current source. */
const sourceMetadata = computed(() => getService(props.source))
/** Display title for the current source. */
const sourceTitle = computed(() =>
  sourceMetadata.value?.title?.trim() || props.source?.trim() || null,
)
/** URL of the source logo to display. */
const sourceLogo = computed(() =>
  sourceLogoAvailable.value ? sourceMetadata.value?.logo ?? null : null,
)

/** Whether the channel logo is available for display. */
const channelLogoAvailable = ref(true)

/** Collection category built from the channel request descriptor. */
const channelCategory = computed(() => {
  const channel = props.channel

  if (!channel || !props.source) {
    return null
  }

  return normalizeHomeCategory(
    {
      label: channel.name,
      key: channel.name,
      request: channel.request,
    },
    0,
    props.source,
  )
})

/** Internal route opening the channel category. */
const channelRoute = computed(() => {
  const category = channelCategory.value

  return category ? mediaRoute(categoryMediaTarget(category)) : null
})

/** Logo rendered beside the channel name while it loads successfully. */
const channelLogo = computed(() =>
  channelLogoAvailable.value ? props.channel?.logo ?? null : null,
)

/** Resets source logo availability when source prop changes. */
watch(
  () => props.source,
  () => {
    sourceLogoAvailable.value = true
  },
)

/** Resets channel logo availability when the channel changes. */
watch(
  () => props.channel,
  () => {
    channelLogoAvailable.value = true
  },
)

/**
 * Requests toggling between the trailer and the current media player.
 */
function handleTrailerToggle() {
  emit('toggle-trailer')
}

/**
 * Hides the source logo when the image cannot be loaded.
 */
function handleSourceLogoError() {
  sourceLogoAvailable.value = false
}
</script>

<template>
  <aside v-if="entryUrl" class="entry-details__poster-panel">
    <div
      class="entry-details__poster-frame"
      :class="{ 'entry-details__poster-frame--transparent': posterFrameUsesContain }"
    >
      <img
        v-if="posterFrameImageUrl"
        class="entry-details__poster"
        :class="{ 'entry-details__poster--contain': posterFrameUsesContain }"
        :src="posterFrameImageUrl"
        :alt="displayTitle"
      />
      <div v-else class="entry-details__poster-fallback" aria-hidden="true">
        <span>{{ displayTitle }}</span>
      </div>

      <img
        v-if="sourceLogo"
        class="entry-details__source-logo"
        :src="sourceLogo"
        :alt="sourceTitle ? `${sourceTitle} logo` : ''"
        :title="sourceTitle ?? undefined"
        @error="handleSourceLogoError"
      />
    </div>

    <div class="entry-details__actions">
      <button
        v-if="showTrailerAction"
        class="entry-details__secondary-action"
        type="button"
        @click="handleTrailerToggle"
      >
        {{ trailerActionLabel }}
      </button>

      <a
        v-if="showSourceEntryAction"
        class="entry-details__primary-action"
        :href="entryUrl"
        target="_blank"
        rel="noreferrer"
      >
        <v-icon icon="mdi-open-in-new" size="18" />
        {{ sourceTitle ? t('entry.sourceEntryNamed', { source: sourceTitle }) : t('entry.sourceEntry') }}
      </a>

      <RouterLink
        v-if="channelRoute && props.channel"
        class="entry-details__secondary-action"
        :to="channelRoute"
        :aria-label="t('entry.channelEntry', { channel: props.channel.name })"
      >
        <img
          v-if="channelLogo"
          class="entry-details__channel-logo"
          :src="channelLogo"
          alt=""
          aria-hidden="true"
          @error="channelLogoAvailable = false"
        />
        <span class="entry-details__channel-label">{{ props.channel.name }}</span>
        <v-icon icon="mdi-chevron-right" size="18" aria-hidden="true" />
      </RouterLink>
    </div>
  </aside>
</template>

<style scoped>
.entry-details__poster-panel {
  display: grid;
  align-content: start;
  gap: 18px;
}

.entry-details__poster-frame {
  position: relative;
  overflow: hidden;
  border-radius: var(--radius);
  background: var(--bg-surface);
  box-shadow: var(--shadow-poster);
  aspect-ratio: 1.5;
}

.entry-details__poster-frame--transparent {
  background: var(--bg-surface);
}

.entry-details__poster {
  width: 100%;
  height: 100%;
  display: block;
  object-fit: contain;
  border-radius: var(--radius);
}

.entry-details__poster--contain {
  padding: 28px;
  object-fit: contain;
  object-position: center;
}

.entry-details__poster-fallback {
  display: grid;
  place-items: center;
  width: 100%;
  height: 100%;
  padding: 24px;
  background:
    radial-gradient(circle at top, var(--color-primary), transparent 36%),
    linear-gradient(180deg, rgba(16, 20, 28, 0.94), rgba(8, 10, 14, 0.98));
  color: var(--text-primary);
  text-align: center;
  font-size: 1.35rem;
  font-weight: 800;
}

.entry-details__actions {
  display: flex;
  flex-direction: column;
  gap: 12px;
  align-items: center;
  justify-content: center;
}

.entry-details__secondary-action,
.entry-details__primary-action {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  min-height: var(--control-height);
  padding: 0 20px;
  border-radius: 999px;
  color: var(--text-primary);
  font-size: 1rem;
  font-weight: 800;
  gap: 10px;
  transition:
    transform var(--duration-fast) ease,
    filter var(--duration-fast) ease;
}

.entry-details__secondary-action {
  border: 1px solid var(--border-color-primary);
  background: var(--bg-accent-primary);
  box-shadow: var(--box-shadow-elevated);
  cursor: pointer;
  text-decoration: none;
}

.entry-details__primary-action {
  background: var(--bg-accent-red);
  text-decoration: none;
  box-shadow: var(--box-shadow-elevated);
}

.entry-details__channel-logo {
  width: 28px;
  height: 28px;
  flex: none;
  object-fit: contain;
}

.entry-details__channel-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.entry-details__source-logo {
  position: absolute;
  top: 10px;
  right: 10px;
  width: auto;
  height: 42px;
  padding: 5px;
  object-fit: contain;
}

.entry-details__secondary-action:hover,
.entry-details__primary-action:hover {
  transform: translateY(-1px);
  filter: brightness(1.04);
}

.entry-details__secondary-action:focus-visible,
.entry-details__primary-action:focus-visible {
  outline: var(--common-focus-ring-danger);
  outline-offset: 2px;
}

@media (max-width: 1120px) {
  .entry-details__poster-panel {
    max-width: 360px;
  }
}
</style>
