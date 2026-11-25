<script setup lang="ts">
import { computed, toRef, useTemplateRef, watch, type DeepReadonly } from 'vue'

import { useMediaCardPreviewPosition } from '@/composables/media-card/useMediaCardPreviewPosition'
import { useI18n } from '@/i18n'
import { formatLanguageLabel } from '@/services/languages'
import type { ServiceMetadata } from '@/types/serviceMetadata'

/**
 * Props accepted by the source card preview popup.
 */
interface Props {
  /** Source rendered by the anchor card. */
  source: DeepReadonly<ServiceMetadata>
  /** Indicates whether the preview should currently be visible. */
  isPreviewOpen: boolean
  /** Root element of the source card used as the popup anchor. */
  anchorRef: HTMLElement | null
}

/** Component props without defaults. */
const props = defineProps<Props>()

const emit = defineEmits<{
  /** Emitted when the popup root element changes. */
  popupRootChange: [element: HTMLElement | null]
}>()

/** Metadata fields displayed first, in this order, when present. */
const METADATA_KEY_ORDER = ['languages', 'geo_countries', 'content_types']

/** Internationalization utilities. */
const { t, tm } = useI18n()
/** Localized metadata field labels. */
const metadataLabels = computed(() => tm('sourceHome.metadataLabels'))
/** Localized content type values. */
const contentTypes = computed(() => tm('sourceHome.contentTypes'))

/** Template reference to the popup root element. */
const popupRef = useTemplateRef<HTMLElement>('popup')

watch(popupRef, (element) => {
  emit('popupRootChange', element)
}, { immediate: true })

/** Reactive reference to the anchor card element. */
const anchor = toRef(props, 'anchorRef')

/** Popup position state computed from the anchor and popup measurements. */
const { position } = useMediaCardPreviewPosition({
  anchorRef: anchor,
  popupRef,
  isOpen: () => props.isPreviewOpen,
})

/**
 * Formats one metadata field with localized values.
 *
 * @param key Metadata field identifier.
 * @param values Raw metadata values to display together.
 * @returns Display value using language and content-type labels when available.
 */
function formatMetadataValues(key: string, values: readonly string[]): string {
  if (key === 'languages') {
    return values.map((value) => formatLanguageLabel(value)).join(', ')
  }

  if (key === 'content_types') {
    return values.map((value) => contentTypes.value[value] ?? value).join(' - ')
  }

  return values.join(' - ')
}

/**
 * Orders metadata fields so the common filters stay stable and readable.
 *
 * @param left First metadata entry.
 * @param right Second metadata entry.
 * @returns Comparison index for the configured field order.
 */
function compareMetadataEntries(
  left: [string, readonly string[]],
  right: [string, readonly string[]],
): number {
  const leftIndex = METADATA_KEY_ORDER.indexOf(left[0])
  const rightIndex = METADATA_KEY_ORDER.indexOf(right[0])
  const leftRank = leftIndex === -1 ? METADATA_KEY_ORDER.length : leftIndex
  const rightRank = rightIndex === -1 ? METADATA_KEY_ORDER.length : rightIndex
  return leftRank - rightRank || left[0].localeCompare(right[0])
}

/** Metadata facts rendered under the service title. */
const metadataFacts = computed(() => Object.entries(props.source.metadata ?? {})
  .filter(([, values]) => values.length > 0)
  .sort(compareMetadataEntries)
  .map(([key, values]) => ({
    key,
    label: metadataLabels.value[key] ?? key,
    value: formatMetadataValues(key, values),
  })))
</script>

<template>
  <Teleport to="body">
    <div
      v-if="isPreviewOpen"
      ref="popup"
      class="source-card-preview"
      :class="`source-card-preview--${position.placement}`"
      :style="{
        left: `${position.left ?? 0}px`,
        top: `${position.top ?? 0}px`,
        visibility: position.visible ? 'visible' : 'hidden',
      }"
      role="tooltip"
    >
      <!-- Small arrow pointing back to the anchor source card. -->
      <span class="source-card-preview__arrow" aria-hidden="true" />
      <div class="source-card-preview__copy">
        <h3 class="source-card-preview__title">
          {{ source.title }}
        </h3>

        <div v-if="metadataFacts.length > 0" class="source-card-preview__facts">
          <p v-for="fact in metadataFacts" :key="fact.key" class="source-card-preview__fact">
            <span class="source-card-preview__fact-label">{{ fact.label }} :</span>
            <span class="source-card-preview__fact-value">{{ fact.value }}</span>
          </p>
        </div>

        <p v-else class="source-card-preview__empty">
          {{ t('sourceHome.noMetadata') }}
        </p>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.source-card-preview {
  position: fixed;
  z-index: 50;
  width: fit-content;
  max-width: min(420px, calc(100vw - 16px));
  max-height: min(480px, calc(100vh - 16px));
  overflow: auto;
  overscroll-behavior: contain;
  scrollbar-width: none;
  /* The popup is display-only: it must not keep itself open nor intercept card interactions. */
  pointer-events: none;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface-strong);
  box-shadow: var(--shadow-heavy);
  backdrop-filter: var(--backdrop-filter-medium);
  color: var(--text-primary);
}

.source-card-preview::-webkit-scrollbar {
  display: none;
}

.source-card-preview__copy {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 15px 14px 14px;
}

/* Tooltip arrow centered on the edge facing the anchor card. */
.source-card-preview__arrow {
  position: absolute;
  display: block;
  width: 10px;
  height: 10px;
  border: inherit;
  background: inherit;
  border-top: 0;
  border-right: 0;
}

.source-card-preview--top .source-card-preview__arrow {
  bottom: -5px;
  left: 50%;
  margin-left: -5px;
  transform: rotate(-45deg);
}

.source-card-preview--bottom .source-card-preview__arrow {
  top: -5px;
  left: 50%;
  margin-left: -5px;
  transform: rotate(135deg);
}

.source-card-preview--left .source-card-preview__arrow {
  top: 50%;
  right: -5px;
  margin-top: -5px;
  transform: rotate(-135deg);
}

.source-card-preview--right .source-card-preview__arrow {
  top: 50%;
  left: -5px;
  margin-top: -5px;
  transform: rotate(45deg);
}

.source-card-preview__title {
  display: block;
  overflow: visible;
  color: var(--text-primary);
  font-size: 1.1rem;
  font-weight: 800;
  line-height: 1.2;
}

.source-card-preview__facts {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px 6px;
}

.source-card-preview__fact {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  overflow: visible;
  color: var(--text-secondary);
  font-size: 0.9rem;
  line-height: 1.3;
  white-space: nowrap;
}

.source-card-preview__fact:not(:last-child)::after {
  content: "•";
  color: var(--text-disabled);
}

.source-card-preview__fact-label {
  color: var(--text-primary);
  font-weight: 700;
}

.source-card-preview__fact-value {
  color: var(--text-secondary);
}

.source-card-preview__empty {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.9rem;
}
</style>
