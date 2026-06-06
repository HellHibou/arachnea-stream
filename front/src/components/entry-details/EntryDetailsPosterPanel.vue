<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { useServiceMetadata } from '@/composables/useServiceMetadata'
import { useI18n } from '@/i18n'

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
}

const props = defineProps<Props>()
const emit = defineEmits<{
  'toggle-trailer': []
}>()

const sourceLogoAvailable = ref(true)
const { getService } = useServiceMetadata()
const { t } = useI18n()

const sourceMetadata = computed(() => getService(props.source))
const sourceTitle = computed(() =>
  sourceMetadata.value?.title?.trim() || props.source?.trim() || null,
)
const sourceLogo = computed(() =>
  sourceLogoAvailable.value ? sourceMetadata.value?.logo ?? null : null,
)

watch(
  () => props.source,
  () => {
    sourceLogoAvailable.value = true
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
        class="entry-details__primary-action"
        :href="entryUrl"
        target="_blank"
        rel="noreferrer"
      >
        <v-icon icon="mdi-open-in-new" size="18" />
        {{ sourceTitle ? t('entry.sourceEntryNamed', { source: sourceTitle }) : t('entry.sourceEntry') }}
      </a>
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
  background: var(--bg-accent-blue);
  box-shadow: var(--box-shadow-elevated);
  cursor: pointer;
}

.entry-details__primary-action {
  background: var(--bg-accent-red);
  text-decoration: none;
  box-shadow: var(--box-shadow-elevated);
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
