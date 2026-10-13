<script setup lang="ts">
import { computed, shallowRef, onMounted, useId } from 'vue'
import ParameterSwitch from '@/components/parameters/ParameterSwitch.vue'
import ParametersPanel from '@/components/parameters/ParametersPanel.vue'
import ParametersSection from '@/components/parameters/ParametersSection.vue'
import { useServiceMetadata } from '@/composables/useServiceMetadata'
import CustomHomeContentPicker from '@/components/home/CustomHomeContentPicker.vue'
import CustomHomeBlockList from '@/components/home/CustomHomeBlockList.vue'
import CustomHomeDraftActions from '@/components/home/CustomHomeDraftActions.vue'
import { validateCustomHomeDefinition } from '@/services/customHomeValidation'
import { applyCustomHomeSelection, moveCustomHomeItem } from '@/services/customHomeDraft'
import { moveCustomHomeBlock, updateCustomHomeGroupDisplay } from '@/services/customHomeBlockGroups'
import { useI18n } from '@/i18n'
import type { CustomHomeDefinition, CustomHomeSelection } from '@/types/customHome'

/** Initial detached draft; remount for a different editing session. Busy/errors belong to persistence owner. */
const props = withDefaults(defineProps<{ definition: CustomHomeDefinition; busy?: boolean; errorMessage?: string | null }>(), {
  busy: false, errorMessage: null,
})
const emit = defineEmits<{ save: [definition: CustomHomeDefinition]; cancel: [] }>()
const metadata = useServiceMetadata()
const switchId = useId()
onMounted(() => { void metadata.load() })
const { t } = useI18n()
const draft = shallowRef(validateCustomHomeDefinition(props.definition))
const validationError = shallowRef<string | null>(null)
const canSave = computed(() => Boolean(draft.value.name.trim()) && !props.busy)

/** @param patch Declarative draft changes without persistence. */
function update(patch: Partial<CustomHomeDefinition>): void {
  draft.value = { ...draft.value, ...patch }
  validationError.value = null
}
/** @param selection Stable picker choice added only to the detached draft. */
function select(selection: CustomHomeSelection): void {
  try {
    update(applyCustomHomeSelection(draft.value, selection))
  } catch (error) { validationError.value = error instanceof Error ? error.message : String(error) }
}
/** Validates and emits the detached draft for atomic persistence by its owner. */
function save(): void {
  try {
    const value = validateCustomHomeDefinition(draft.value)
    emit('save', value)
  } catch (error) { validationError.value = error instanceof Error ? error.message : String(error) }
}
/** @param id Block identity. @param title User display override (blank removes it). */
function updateTitle(id: string, title: string): void {
  update({ blocks: draft.value.blocks.map((block) => block.id === id ? { ...block, titleOverride: title.trim() || null } : block) })
}
/** @param id Stable block id. @param options Detached display defaults saved only with the definition. */
function updateDisplay(id: string, options: import('@/types/customHome').CustomHomeDisplayOptions): void {
  update({ blocks: updateCustomHomeGroupDisplay(draft.value.blocks, draft.value.sectionMergeMode === 'title', id, options) })
}
/** @param id Member identity. @param direction Movement. @param group Move the complete title group. */
function moveBlock(id: string, direction: -1 | 1, group = false): void {
  update({ blocks: moveCustomHomeBlock(draft.value.blocks, draft.value.sectionMergeMode === 'title', id, direction, group) })
}
defineExpose({ save, canSave })
</script>

<template>
  <section class="custom-home-editor">
    <v-alert v-if="props.errorMessage || validationError" type="error" role="alert">{{ validationError ?? props.errorMessage }}</v-alert>
    <fieldset :disabled="props.busy" class="custom-home-editor__fields">
      <ParametersSection :title="t('customHome.homeGroup')">
      <label class="custom-home-editor__label">{{ t('customHome.name') }}
        <input class="custom-home-editor__input" :value="draft.name" type="text"
          @input="update({ name: ($event.target as HTMLInputElement).value })">
      </label>
      <div class="custom-home-editor__switches">
        <ParametersPanel :title="t('customHome.mergeTitles')">
          <ParameterSwitch :input-id="`${switchId}-merge`" :leading-label="t('settings.inactive')"
            :trailing-label="t('settings.active')" :checked="draft.sectionMergeMode === 'title'" :disabled="props.busy"
            @update:checked="update({ sectionMergeMode: $event ? 'title' : 'none' })" />
        </ParametersPanel>
        <ParametersPanel :title="t('customHome.showBookmarks')">
          <ParameterSwitch :input-id="`${switchId}-bookmarks`" :leading-label="t('settings.hidden')"
            :trailing-label="t('settings.visible')" :checked="draft.showBookmarks" :disabled="props.busy"
            @update:checked="update({ showBookmarks: $event })" />
        </ParametersPanel>
      </div>
      </ParametersSection>
      <ParametersSection :title="t('customHome.banners')">
      <div class="custom-home-editor__table-wrap">
      <table class="custom-home-editor__table">
        <thead><tr><th scope="col">{{ t('customHome.serviceColumn') }}</th><th scope="col">{{ t('customHome.actionsColumn') }}</th></tr></thead>
        <tbody><tr v-for="(source, index) in draft.bannerSources" :key="source">
        <td>{{ metadata.getService(source)?.title ?? source }}</td>
        <td>
        <CustomHomeDraftActions :index="index" :count="draft.bannerSources.length"
          @move="update({ bannerSources: moveCustomHomeItem(draft.bannerSources, index, $event) })"
          @remove="update({ bannerSources: draft.bannerSources.filter((id) => id !== source) })" />
        </td></tr></tbody>
      </table></div>
      </ParametersSection>
      <ParametersSection :title="t('customHome.shortcuts')">
      <div class="custom-home-editor__table-wrap">
      <table class="custom-home-editor__table">
        <thead><tr><th scope="col">{{ t('customHome.serviceColumn') }}</th><th scope="col">{{ t('customHome.contentColumn') }}</th><th scope="col">{{ t('customHome.actionsColumn') }}</th></tr></thead>
        <tbody><tr v-for="(reference, index) in draft.categoryShortcuts" :key="`${reference.sourceId}:${reference.categoryKey}:${index}`">
        <td>{{ metadata.getService(reference.sourceId)?.title ?? reference.sourceId }}</td><td>{{ reference.labelSnapshot }}</td>
        <td>
        <CustomHomeDraftActions :index="index" :count="draft.categoryShortcuts.length"
          @move="update({ categoryShortcuts: moveCustomHomeItem(draft.categoryShortcuts, index, $event) })"
          @remove="update({ categoryShortcuts: draft.categoryShortcuts.filter((_, position) => position !== index) })" />
        </td></tr></tbody>
      </table></div>
      </ParametersSection>
      <CustomHomeBlockList :blocks="draft.blocks" :merge-titles="draft.sectionMergeMode === 'title'" @title="updateTitle"
        @display="updateDisplay"
        @move="moveBlock"
        @move-group="(id, direction) => moveBlock(id, direction, true)"
        @remove="(id) => update({ blocks: draft.blocks.filter((block) => block.id !== id) })" />
      <CustomHomeContentPicker @select="select" />
    </fieldset>
  </section>
</template>

<style scoped>
.custom-home-editor {
  --parameters-control-width: 240px;
  display: grid;
  gap: 8px;
  color: var(--text-primary);
  font: inherit;
}

.custom-home-editor__fields {
  display: grid;
  gap: 12px;
  min-width: 0;
  border: 0;
}

.custom-home-editor__switches {
  display: grid;
  gap: 2px;
}


.custom-home-editor__table-wrap {
  overflow-x: auto;
}

.custom-home-editor__table {
  width: 100%;
  border-collapse: collapse;
}

.custom-home-editor__table th,
.custom-home-editor__table td {
  padding: 8px;
  border-bottom: 1px solid var(--border-color-primary);
  text-align: left;
  vertical-align: middle;
}


.custom-home-editor__table th:last-child,
.custom-home-editor__table td:last-child {
  width: 1%;
  text-align: right;
  white-space: nowrap;
}


.custom-home-editor__label {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-left: 10px;
  padding: 1px 0;
  white-space: nowrap;
  font-size: 0.92rem;
  font-weight: 700;
}

.custom-home-editor__input {
  flex: 1;
  min-width: 0;
  padding: 8px 14px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-transparent);
  color: var(--text-primary);
  font: inherit;
}

.custom-home-editor__input:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>