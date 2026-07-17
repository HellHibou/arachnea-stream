<script setup lang="ts">
import { computed } from 'vue'
import type { MediaItem } from '@/types/media'
import { useI18n } from '@/i18n'

import MediaCardCollection from '../MediaCardCollection.vue'

/**
 * Props accepted by the generic selectable catalog section rendered below the metadata.
 */
interface Props {
  groupItems: MediaItem[]
  selectedGroupLabel: string
  showItemSection: boolean
  groupErrorMessage: string | null
  isGroupLoading: boolean
  showGroupPrompt: boolean
  showEmptyGroupState: boolean
  displayedItems: MediaItem[]
  displayedItemLabel: string
  showLoadMoreItems: boolean
  isLoadingMoreItems: boolean
  groupSectionLabel?: string
  itemSectionLabel?: string
  selectFieldLabel?: string
  stateEyebrow?: string
  loadingStateTitle?: string
  loadingStateMessage?: string
  promptTitle?: string
  promptMessage?: string
  emptyStateTitle?: string
  emptyStateMessage?: string
  loadMoreLabel?: string
  showServiceLogo?: boolean
}

/** Component props with applied defaults. */
const props = withDefaults(defineProps<Props>(), {
  showServiceLogo: true,
})
/** Internationalization utilities. */
const { t } = useI18n()

/** Resolved label for the group section. */
const groupSectionLabel = computed(() => props.groupSectionLabel ?? t('entry.group'))
/** Resolved label for the item section. */
const itemSectionLabel = computed(() => props.itemSectionLabel ?? t('entry.contents'))
/** Resolved label for the select field. */
const selectFieldLabel = computed(() => props.selectFieldLabel ?? t('entry.group'))
/** Resolved label for the state eyebrow. */
const stateEyebrow = computed(() => props.stateEyebrow ?? t('entry.group'))
/** Resolved title for the loading state. */
const loadingStateTitle = computed(() => props.loadingStateTitle ?? t('entry.loadingTitle'))
/** Resolved message for the loading state. */
const loadingStateMessage = computed(() => props.loadingStateMessage ?? t('entry.loadingMessage'))
/** Resolved title for the prompt state. */
const promptTitle = computed(() => props.promptTitle ?? t('entry.promptTitle'))
/** Resolved message for the prompt state. */
const promptMessage = computed(() => props.promptMessage ?? t('entry.promptMessage'))
/** Resolved title for the empty state. */
const emptyStateTitle = computed(() => props.emptyStateTitle ?? t('entry.emptyContentTitle'))
/** Resolved message for the empty state. */
const emptyStateMessage = computed(() => props.emptyStateMessage ?? t('entry.emptyContentMessage'))
/** Resolved label for the load more button. */
const loadMoreLabel = computed(() => props.loadMoreLabel ?? t('catalog.loadMore'))

const emit = defineEmits<{
  /** Emitted when a group is selected. */
  'select-group': [item: MediaItem]
  /** Emitted when an item is selected. */
  'select-item': [item: MediaItem]
  /** Emitted when more items should be loaded. */
  'load-more': []
}>()

/**
 * Forwards the group selection to the parent details controller.
 *
 * @param title Title of the selected group.
 */
function handleGroupSelectFromTitle(title: string) {
  const group = props.groupItems.find((item) => item.title === title)
  if (group) {
    emit('select-group', group)
  }
}

/**
 * Forwards the item selection to the parent details controller.
 *
 * @param item Selected media card.
 */
function handleItemSelect(item: MediaItem) {
  emit('select-item', item)
}

/**
 * Requests loading more items for the current list.
 */
function handleLoadMore() {
  emit('load-more')
}
</script>

<template>
  <section class="entry-details-episodes">
    <section v-if="props.groupItems.length === 1" class="entry-details__season-single">
      <p class="entry-details__season-single-eyebrow">{{ groupSectionLabel }}</p>
      <h2 class="entry-details__season-single-title">
        {{ props.groupItems[0]?.title }}
      </h2>
    </section>

    <section v-else-if="props.groupItems.length > 1" class="entry-details__seasons">
      <p class="entry-details__season-single-eyebrow">{{ groupSectionLabel }}</p>
      <label class="entry-details__select-field" for="entry-group-select">
        <span class="entry-details__select-label">{{ selectFieldLabel }}</span>
        <select
          id="entry-group-select"
          class="entry-details__select"
          :value="props.selectedGroupLabel"
          @change="handleGroupSelectFromTitle(($event.target as HTMLSelectElement).value)"
        >
          <option
            v-for="group in props.groupItems"
            :key="group.id"
            :value="group.title"
          >
            {{ group.title }}
          </option>
        </select>
      </label>
    </section>

    <section v-if="props.showItemSection" class="entry-details__episodes">
      <p class="entry-details__season-single-eyebrow">{{ itemSectionLabel }}</p>
      <div v-if="props.groupErrorMessage" class="entry-details__season-state">
        <p class="entry-details__season-state-eyebrow">{{ stateEyebrow }}</p>
        <h2 class="entry-details__season-state-title">{{ t('catalog.loadingErrorTitle') }}</h2>
        <p class="entry-details__season-state-copy">
          {{ props.groupErrorMessage }}
        </p>
      </div>

      <div v-else-if="props.isGroupLoading" class="entry-details__season-state">
        <p class="entry-details__season-state-eyebrow">{{ stateEyebrow }}</p>
        <h2 class="entry-details__season-state-title">{{ loadingStateTitle }}</h2>
        <p class="entry-details__season-state-copy">
          {{ loadingStateMessage }}
        </p>
      </div>

      <div v-else-if="props.showGroupPrompt" class="entry-details__season-state">
        <p class="entry-details__season-state-eyebrow">{{ stateEyebrow }}</p>
        <h2 class="entry-details__season-state-title">{{ promptTitle }}</h2>
        <p class="entry-details__season-state-copy">
          {{ promptMessage }}
        </p>
      </div>

      <div v-else-if="props.showEmptyGroupState" class="entry-details__season-state">
        <p class="entry-details__season-state-eyebrow">{{ stateEyebrow }}</p>
        <h2 class="entry-details__season-state-title">{{ emptyStateTitle }}</h2>
        <p class="entry-details__season-state-copy">
          {{ emptyStateMessage }}
        </p>
      </div>

      <MediaCardCollection
        v-else
        :items="props.displayedItems"
        mode="list"
        thumbnail-orientation="landscape"
        thumbnail-image-fit="cover"
        :hide-missing-list-thumbnails="true"
        :show-service-logo="props.showServiceLogo"
        @select="handleItemSelect"
      />

      <div v-if="props.showLoadMoreItems" class="entry-details__pagination">
        <button
          :class="
            props.isLoadingMoreItems
              ? 'entry-details__pagination-loading'
              : 'entry-details__pagination-action'
          "
          type="button"
          :disabled="props.isLoadingMoreItems"
          @click="handleLoadMore"
        >
          {{ props.isLoadingMoreItems ? t('catalog.loading') : loadMoreLabel }}
        </button>
      </div>
    </section>
  </section>
</template>

<style scoped>
.entry-details-episodes {
  display: grid;
  gap: 18px;
}

.entry-details__season-single {
  display: grid;
  gap: 6px;
  padding: 4px 0;
}

.entry-details__season-single-eyebrow {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.82rem;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}

.entry-details__season-single-title {
  margin: 0;
  color: var(--text-primary);
  font-size: clamp(1.2rem, 1.08rem + 0.4vw, 1.45rem);
  font-weight: 700;
}

.entry-details__select-field {
  display: block;
}

.entry-details__select-label {
  position: absolute;
  width: 1px;
  height: 1px;
  margin: -1px;
  padding: 0;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}

.entry-details__select {
  min-width: 200px;
  min-height: 40px;
  padding: 10px 14px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
  font-size: clamp(1.2rem, 1.08rem + 0.4vw, 1.45rem);
  font-weight: 700;
  cursor: pointer;
  transition:
    border-color var(--duration-fast) ease,
    background-color var(--duration-fast) ease;
}

.entry-details__select:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.entry-details__select:hover {
  border-color: var(--color-primary);
}

.entry-details__episodes {
  padding: 10px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  backdrop-filter: var(--backdrop-filter-strong);
}

.entry-details__episodes :deep(.media-card-collection-section) {
  gap: 14px;
}

.entry-details__episodes :deep(.media-card-collection__title) {
  font-size: clamp(1.2rem, 1.1rem + 0.45vw, 1.5rem);
}

.entry-details__season-state {
  display: grid;
  gap: 10px;
  padding: 18px 20px;
  border-radius: var(--radius);
  background: var(--bg-surface);
  box-shadow: var(--inset-light);
}

.entry-details__season-state-eyebrow {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.82rem;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}

.entry-details__season-state-title {
  margin: 0;
  color: var(--text-primary);
  font-size: clamp(1.2rem, 1.08rem + 0.4vw, 1.45rem);
}

.entry-details__season-state-copy {
  margin: 0;
  color: var(--text-secondary);
  line-height: 1.6;
}

.entry-details__pagination {
  display: flex;
  justify-content: center;
  margin-top: 25px;
}

.entry-details__pagination-loading,
.entry-details__pagination-action {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: var(--control-height);
  padding: 0 20px;
  color: var(--text-primary);
  font-size: 1rem;
  font-weight: 800;
  cursor: pointer;
  transition:
    transform var(--duration-fast) ease,
    filter var(--duration-fast) ease,
    opacity var(--duration-fast) ease;
}

.entry-details__pagination-loading {
  background: var(--bg-surface);
}

.entry-details__pagination-action {
  background: var(--bg-accent-primary);
  box-shadow: var(--box-shadow-elevated);
  border: 1px solid var(--border-color-primary);
  border-radius: 999px;
}

.entry-details__pagination-action:disabled {
  cursor: wait;
  opacity: 0.72;
}

.entry-details__pagination-action:hover:not(:disabled) {
  transform: translateY(-1px);
  filter: brightness(1.06);
}

.entry-details__pagination-action:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>
