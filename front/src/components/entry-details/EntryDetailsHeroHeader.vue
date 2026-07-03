<script setup lang="ts">
import { useI18n } from '@/i18n'

/**
 * Props accepted by the headline rendered above the embedded player.
 */
interface Props {
  displayTitle: string
  alternativeTitleLabel: string | null
  selectedPlayableTitle: string | null
  showAdjacentNavigation: boolean
  hasPreviousPlayable: boolean
  hasNextPlayable: boolean
  showBookmarkAction: boolean
  isBookmarked: boolean
}

/** Component props without defaults. */
defineProps<Props>()

const emit = defineEmits<{
  /** Emitted when navigating to adjacent playable entries. */
  'step-playable': [offset: -1 | 1]
  /** Emitted when the bookmark toggle is clicked. */
  'toggle-bookmark': []
}>()
/** Internationalization utilities. */
const { t } = useI18n()

/**
 * Requests selecting the previous or next playable item.
 *
 * @param offset Relative item offset to apply.
 */
function handlePlayableStep(offset: -1 | 1) {
  emit('step-playable', offset)
}

/**
 * Toggles the bookmark stored for the current entry.
 */
function handleBookmarkToggle() {
  emit('toggle-bookmark')
}
</script>

<template>
  <div class="entry-details__headline-copy">
    <div class="entry-details__title-row">
      <button
        v-if="showBookmarkAction"
        class="entry-details__bookmark-button"
        :class="{ 'entry-details__bookmark-button--active': isBookmarked }"
        type="button"
        :aria-pressed="isBookmarked"
        :title="isBookmarked ? t('entry.removeBookmark') : t('entry.addBookmark')"
        @click="handleBookmarkToggle"
      >
        <v-icon
          :icon="isBookmarked ? 'mdi-bookmark' : 'mdi-bookmark-outline'"
          size="24"
          aria-hidden="true"
        />
      </button>

      <h1 id="title-section" class="entry-details__title">
        {{ displayTitle }}
      </h1>
    </div>

    <p v-if="alternativeTitleLabel" class="entry-details__alternative-title">
      <em class="entry-details__alternative-title-value">{{ alternativeTitleLabel }}</em>
    </p>

    <div
      v-if="showAdjacentNavigation && selectedPlayableTitle"
      class="entry-details__player-episode-header"
    >
       <button
         class="entry-details__episode-nav-button"
         :class="{ 'entry-details__episode-nav-button--hidden': !hasPreviousPlayable }"
         type="button"
         :disabled="!hasPreviousPlayable"
         :aria-label="t('entry.previousContent')"
         @click="handlePlayableStep(-1)"
       >
         <v-icon icon="mdi-chevron-left" size="20" aria-hidden="true" />
       </button>

      <p class="entry-details__player-episode-title">
        {{ selectedPlayableTitle }}
      </p>

       <button
         class="entry-details__episode-nav-button"
         :class="{ 'entry-details__episode-nav-button--hidden': !hasNextPlayable }"
         type="button"
         :disabled="!hasNextPlayable"
         :aria-label="t('entry.nextContent')"
         @click="handlePlayableStep(1)"
       >
         <v-icon icon="mdi-chevron-right" size="20" aria-hidden="true" />
       </button>
    </div>
  </div>
</template>

<style scoped>
.entry-details__headline-copy {
  display: grid;
  gap: 12px;
}

.entry-details__title-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.entry-details__title {
  margin: 0;
  font-size: clamp(2rem, 1.7rem + 1.3vw, 3.1rem);
  font-weight: 900;
  line-height: 1.02;
  text-shadow: var(--text-shadow-primary);
}

.mdi-bookmark-outline,
.mdi-bookmark {
  color: var(--color-primary);
}

.entry-details__bookmark-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 42px;
  min-width: 42px;
  height: 42px;
  padding: 0;
  border: 1px solid var(--border-color-primary);
  border-radius: 999px;
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
  font-size: 0.95rem;
  font-weight: 800;
  cursor: pointer;
  transition:
    transform var(--duration-fast) ease,
    filter var(--duration-fast) ease,
    background-color var(--duration-fast) ease,
    border-color var(--duration-fast) ease;
}

.entry-details__bookmark-button:hover {
  transform: translateY(-1px);
  filter: brightness(1.06);
}

.entry-details__bookmark-button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.entry-details__alternative-title {
  display: grid;
  gap: 4px;
}

.entry-details__alternative-title-value {
  color: var(--text-primary);
  font-size: 1rem;
  line-height: 1.4;
  text-shadow: var(--text-shadow-primary);
}

.entry-details__player-episode-header {
  display: grid;
  grid-template-columns: 44px minmax(0, 1fr) 44px;
  align-items: center;
  gap: 12px;
  max-width: 880px;
}

.entry-details__player-episode-title {
  text-align: center;
  color: var(--text-primary);
  font-size: 1rem;
  font-weight: 700;
  line-height: 1.4;
  text-shadow: var(--text-shadow-primary);
}

.entry-details__episode-nav-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 44px;
  min-width: 44px;
  height: 44px;
  padding: 0;
  border: 1px solid var(--border-color-primary);
  border-radius: 999px;
  background: var(--bg-surface);
  color: var(--text-primary);
  font-size: 1rem;
  font-weight: 800;
  cursor: pointer;
  transition:
    transform var(--duration-fast) ease,
    filter var(--duration-fast) ease,
    opacity var(--duration-fast) ease;
}

.entry-details__episode-nav-button:hover:not(:disabled) {
  transform: translateY(-1px);
  filter: brightness(1.06);
}

.entry-details__episode-nav-button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.entry-details__episode-nav-button--hidden {
  visibility: hidden;
  pointer-events: none;
}

@media (max-width: 720px) {
  .entry-details__title-row {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
