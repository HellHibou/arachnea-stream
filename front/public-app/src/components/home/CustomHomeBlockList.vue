<script setup lang="ts">
import type { CustomHomeBlock } from '@/types/customHome'
import ParametersSection from '@/components/parameters/ParametersSection.vue'
import { useI18n } from '@/i18n'
import { onMounted } from 'vue'
import { useServiceMetadata } from '@/composables/useServiceMetadata'
import CustomHomeDraftActions from './CustomHomeDraftActions.vue'
import HomeSectionActions from './HomeSectionActions.vue'
import type { CustomHomeDisplayOptions } from '@/types/customHome'
import type { HomeSection } from '@/types/home'
/** Ordered detached blocks; all mutations are emitted to the draft owner. */
defineProps<{ blocks: CustomHomeBlock[] }>()
const emit = defineEmits<{ move: [index: number, direction: -1 | 1]; remove: [id: string];
  title: [id: string, title: string]; display: [id: string, options: CustomHomeDisplayOptions] }>()
const { t } = useI18n()
const metadata = useServiceMetadata()
onMounted(() => { void metadata.load() })
/** @param block Detached row. @returns Minimal descriptor for the shared display-only menu. */
function displaySection(block: CustomHomeBlock): HomeSection {
  return { id: block.id, preferenceKey: block.id, label: block.titleOverride ?? block.reference.labelSnapshot,
    items: [], sources: [], sourceOrder: [block.reference.sourceId], currentPage: 1, haveMore: false }
}
/** @param block Detached row. @param options Changed display defaults, never written directly to storage. */
function updateDisplay(block: CustomHomeBlock, options: CustomHomeDisplayOptions): void {
  emit('display', block.id, { ...block.displayOptions, ...options })
}
</script>

<template>
  <ParametersSection :title="t('customHome.blocks')">
  <div class="custom-home-blocks">
    <table class="custom-home-blocks__table">
      <caption class="custom-home-blocks__caption">{{ t('customHome.blocks') }}</caption>
      <thead><tr>
        <th scope="col">{{ t('customHome.serviceColumn') }}</th>
        <th scope="col">{{ t('customHome.contentColumn') }}</th>
        <th scope="col">{{ t('customHome.titleOverride') }}</th>
        <th scope="col">{{ t('customHome.actionsColumn') }}</th>
      </tr></thead>
      <tbody><tr v-for="(block, index) in blocks" :key="block.id">
        <td>{{ metadata.getService(block.reference.sourceId)?.title ?? block.reference.sourceId }}</td>
        <td>{{ block.reference.labelSnapshot ?? t('customHome.untitled') }}
          <small class="custom-home-blocks__kind">{{ t(block.kind === 'home-section' ? 'customHome.section' : 'customHome.categoryContent') }}</small>
        </td>
        <td><input class="custom-home-blocks__input" type="text" :value="block.titleOverride ?? ''"
          :aria-label="`${t('customHome.titleOverride')}: ${block.reference.labelSnapshot ?? t('customHome.untitled')}`"
          @input="emit('title', block.id, ($event.target as HTMLInputElement).value)"></td>
        <td><CustomHomeDraftActions :index="index" :count="blocks.length"
          @move="emit('move', index, $event)" @remove="emit('remove', block.id)" />
          <HomeSectionActions :section="displaySection(block)" :is-pinned="false" hide-pin-button hide-move-buttons
            :collection-mode="block.displayOptions?.collectionMode ?? 'single-row'"
            :thumbnail-orientation="block.displayOptions?.thumbnailOrientation ?? 'portrait'"
            :thumbnail-image-fit="block.displayOptions?.thumbnailImageFit ?? 'cover'"
            @update-collection-mode="(_key, value) => { if (value) updateDisplay(block, { collectionMode: value }) }"
            @update-thumbnail-orientation="(_key, value) => { if (value) updateDisplay(block, { thumbnailOrientation: value }) }"
            @update-thumbnail-image-fit="(_key, value) => { if (value) updateDisplay(block, { thumbnailImageFit: value }) }" />
        </td>
      </tr></tbody>
    </table>
  </div>
  </ParametersSection>
</template>

<style scoped>
.custom-home-blocks {
  overflow-x: auto;
}

.custom-home-blocks__table {
  width: 100%;
  min-width: 650px;
  border-collapse: collapse;
}

.custom-home-blocks__caption {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
}

.custom-home-blocks__table th,
.custom-home-blocks__table td {
  padding: 8px;
  border-bottom: 1px solid var(--border-color-primary);
  text-align: left;
  vertical-align: middle;
}

.custom-home-blocks__kind {
  display: block;
  color: var(--text-secondary);
}

.custom-home-blocks__table th:last-child,
.custom-home-blocks__table td:last-child {
  width: 1%;
  text-align: right;
  white-space: nowrap;
}

.custom-home-blocks__input {
  width: 100%;
  min-width: 180px;
  padding: 8px 12px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-transparent);
  color: var(--text-primary);
  font: inherit;
}

.custom-home-blocks__input:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>