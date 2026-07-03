<script setup lang="ts">
import { useI18n } from '@/i18n'
/** Internationalization utilities. */
const { t } = useI18n()

/**
 * Props accepted by the metadata section rendered below the hero area.
 */
interface Props {
  /** List of metadata badges to display. */
  metadataBadges: string[]
  /** List of topic chips to display. */
  topicChips: string[]
  /** Genre text to display. */
  genreText: string | null
  /** Year label to display. */
  yearLabel: string | null
  /** Release date label to display. */
  displayReleaseDateLabel: string | null
  /** Expiration date label to display. */
  displayExpireLabel: string | null
  /** Content advisor label to display. */
  contentAdvisorLabel: string | null
  /** Audio language label to display. */
  audioLanguageLabel: string | null
  /** Subtitle language label to display. */
  subtitleLanguageLabel: string | null
  /** Duration label to display. */
  displayDurationLabel: string | null
  /** Casting text to display. */
  castingText: string | null
  /** Director text to display. */
  directorText: string | null
  /** Score to display. */
  score: number | null
}

/** Component props without defaults. */
defineProps<Props>()
</script>

<template>
  <section class="entry-details-metadata">
    <div v-if="metadataBadges.length" class="entry-details__badge-row">
      <span
        v-for="badge in metadataBadges"
        :key="badge"
        class="entry-details__badge"
      >
        {{ badge }}
      </span>
    </div>

    <div v-if="topicChips.length" class="entry-details__chip-row">
      <span v-for="chip in topicChips" :key="chip" class="entry-details__chip">
        {{ chip }}
      </span>
    </div>

    <dl class="entry-details__facts">
      <div v-if="genreText" class="entry-details__fact">
        <dt>{{ t('media.theme') }}</dt>
        <dd>{{ genreText }}</dd>
      </div>

      <div v-if="yearLabel" class="entry-details__fact">
        <dt>{{ t('media.year') }}</dt>
        <dd>{{ yearLabel }}</dd>
      </div>

      <div v-if="score !== null" class="entry-details__fact">
        <dt>{{ t('media.score') }}</dt>
        <dd>{{ score }}</dd>
      </div>

      <div v-if="displayReleaseDateLabel" class="entry-details__fact">
        <dt>{{ t('media.releaseDate') }}</dt>
        <dd>{{ displayReleaseDateLabel }}</dd>
      </div>

      <div v-if="displayExpireLabel" class="entry-details__fact">
        <dt>{{ t('media.availableUntil') }}</dt>
        <dd>{{ displayExpireLabel }}</dd>
      </div>

      <div v-if="contentAdvisorLabel" class="entry-details__fact">
        <dt>{{ t('media.contentAdvisor') }}</dt>
        <dd>{{ contentAdvisorLabel }}</dd>
      </div>

      <div v-if="audioLanguageLabel" class="entry-details__fact">
        <dt>{{ t('media.audioLanguage') }}</dt>
        <dd>{{ audioLanguageLabel }}</dd>
      </div>

      <div v-if="subtitleLanguageLabel" class="entry-details__fact">
        <dt>{{ t('media.subtitleLanguage') }}</dt>
        <dd>{{ subtitleLanguageLabel }}</dd>
      </div>

      <div v-if="displayDurationLabel" class="entry-details__fact">
        <dt>{{ t('duration') }}</dt>
        <dd>{{ displayDurationLabel }}</dd>
      </div>

      <div v-if="castingText" class="entry-details__fact">
        <dt>{{ t('media.cast') }}</dt>
        <dd>{{ castingText }}</dd>
      </div>

      <div v-if="directorText" class="entry-details__fact">
        <dt>{{ t('media.director') }}</dt>
        <dd>{{ directorText }}</dd>
      </div>
    </dl>
  </section>
</template>

<style scoped>
.entry-details-metadata {
  display: grid;
  gap: 18px;
}

.entry-details__badge-row,
.entry-details__chip-row {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}

.entry-details__badge,
.entry-details__chip {
  display: inline-flex;
  align-items: center;
  min-height: 36px;
  padding: 0 14px;
  border: 1px solid var(--border-color-primary);
  border-radius: 999px;
  background: var(--bg-surface);
  color: var(--text-primary);
  font-size: 0.95rem;
  font-weight: 700;
  backdrop-filter: var(--backdrop-filter-medium);
}

.entry-details__chip {
  font-weight: 600;
}

.entry-details__facts {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
}

.entry-details__fact {
  display: grid;
  align-content: start;
  gap: 6px;
  padding: 16px 18px;
  border-radius: var(--radius);
  background: var(--bg-surface);
  box-shadow: var(--inset-light);
}

.entry-details__fact dt {
  color: var(--text-secondary);
  font-size: 0.82rem;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}

.entry-details__fact dd {
  color: var(--text-primary);
  font-size: 1rem;
  font-weight: 700;
  line-height: 1.4;
}

@media (max-width: 720px) {
  .entry-details__facts {
    grid-template-columns: 1fr;
  }
}
</style>
