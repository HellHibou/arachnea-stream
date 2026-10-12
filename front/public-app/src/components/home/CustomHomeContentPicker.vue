<script setup lang="ts">
import { computed, onMounted, shallowRef, watch } from 'vue'
import ParametersSection from '@/components/parameters/ParametersSection.vue'
import { useServiceMetadata } from '@/composables/useServiceMetadata'
import { useCustomHomePicker } from '@/composables/home/useCustomHomePicker'
import { createCategoryReference, createSectionReference } from '@/services/customHomeReferences'
import { useI18n } from '@/i18n'
import type { CustomHomeSelection } from '@/types/customHome'
import type { HomeCategory, HomeSection } from '@/types/home'

/** Restrict candidates for an add intent; empty source list allows all available services. @default [] */
const props = withDefaults(defineProps<{ sourceIds?: string[]; categoryLabel?: string | null;
  sectionLabel?: string | null; blockOnly?: boolean }>(), {
  sourceIds: () => [], categoryLabel: null, sectionLabel: null, blockOnly: false,
})
const emit = defineEmits<{ select: [selection: CustomHomeSelection] }>()
const { t } = useI18n()
const metadata = useServiceMetadata()
const picker = useCustomHomePicker()
const selectedSource = shallowRef('')
const selectionError = shallowRef<string | null>(null)
const choices = computed(() => metadata.services.value.filter((service) =>
  service.supportedQueries.includes('load_home') && (!props.sourceIds.length || props.sourceIds.includes(service.id)))
  .map((service) => ({ title: service.title, value: service.id })))
const categories = computed(() => (props.sectionLabel ? [] : picker.discovery.value?.catalog.categories ?? [])
  .filter((category) => !props.categoryLabel || category.label === props.categoryLabel))
const sections = computed(() => (props.categoryLabel ? [] : picker.discovery.value?.catalog.sections ?? [])
  .filter((section) => !props.sectionLabel || section.label === props.sectionLabel))
watch(selectedSource, (id) => { selectionError.value = null; void picker.discover(id) })
onMounted(() => { void metadata.load() })

/** @param category Unmerged category. @param kind Explicit shortcut or embedded content choice. */
function selectCategory(category: HomeCategory, kind: 'shortcut' | 'block'): void {
  try {
    const reference = createCategoryReference(category)
    emit('select', kind === 'shortcut' ? { kind, reference } : {
      kind, block: { id: crypto.randomUUID(), kind: 'category-content', reference, titleOverride: null },
    })
  } catch (error) { selectionError.value = error instanceof Error ? error.message : String(error) }
}
/** @param section Unmerged section. @param subsectionKey Optional source-owned variant identity. */
function selectSection(section: HomeSection, subsectionKey?: string): void {
  try {
    emit('select', { kind: 'block', block: { id: crypto.randomUUID(), kind: 'home-section',
      reference: createSectionReference(section, subsectionKey), titleOverride: null } })
  } catch (error) { selectionError.value = error instanceof Error ? error.message : String(error) }
}
</script>

<template>
  <ParametersSection :title="t('customHome.source')">
  <div class="custom-home-picker">
    <v-select v-model="selectedSource" :items="choices" :label="t('customHome.serviceColumn')"
      :loading="metadata.isLoading.value" clearable @update:model-value="(value) => { if (!value) selectedSource = '' }" />
    <v-alert v-if="metadata.errorMessage.value || picker.errorMessage.value || selectionError" type="warning">
      {{ selectionError ?? picker.errorMessage.value ?? metadata.errorMessage.value }}
      <v-btn @click="metadata.refresh(); picker.discover(selectedSource)">{{ t('sourceHome.retry') }}</v-btn>
    </v-alert>
    <p v-if="picker.isLoading.value" role="status">{{ t('entry.loadingMessage') }}</p>
    <template v-if="picker.discovery.value">
      <p>{{ metadata.getService(selectedSource)?.title ?? selectedSource }}</p>
      <div class="custom-home-picker__table-wrap">
      <table class="custom-home-picker__table">
        <thead><tr><th scope="col">{{ t('customHome.typeColumn') }}</th><th scope="col">{{ t('customHome.contentColumn') }}</th>
          <th scope="col">{{ t('customHome.actionsColumn') }}</th><th scope="col">{{ t('customHome.actionsColumn') }}</th></tr></thead>
        <tbody>
      <tr v-if="!props.blockOnly && !props.categoryLabel && !props.sectionLabel">
        <td>{{ t('customHome.banners') }}</td><td>{{ metadata.getService(selectedSource)?.title ?? selectedSource }}</td><td>
      <v-btn
        @click="emit('select', { kind: 'banners', sourceId: selectedSource })">{{ t('customHome.addBanners') }}</v-btn>
        </td><td /></tr>
      <tr v-for="category in categories" :key="category.id">
        <td>{{ t('customHome.categoryType') }}</td><td>{{ category.label }}</td><td>
        <v-btn v-if="!props.blockOnly" :disabled="category.identity?.ambiguous || !category.identity"
          @click="selectCategory(category, 'shortcut')">{{ t('customHome.shortcut') }}</v-btn>
        </td><td>
        <v-btn :disabled="category.identity?.ambiguous || !category.identity"
          @click="selectCategory(category, 'block')">{{ t('customHome.categoryContent') }}</v-btn>
        </td></tr>
      <template v-for="section in sections" :key="section.id">
      <tr>
        <td>{{ t('customHome.section') }}</td><td>{{ section.label ?? t('customHome.untitled') }}</td><td>
        <v-btn :disabled="section.identity?.ambiguous || !section.identity"
          @click="selectSection(section)">{{ t('customHome.addSection') }}</v-btn>
        </td><td /></tr>
      <tr v-for="variant in section.subsections ?? []" :key="variant.key">
        <td>{{ t('customHome.section') }}</td><td>{{ section.label }} — {{ variant.label ?? t('customHome.untitled') }}</td><td>
        <v-btn
          :disabled="variant.identity?.ambiguous || !variant.identity"
          @click="selectSection(section, variant.identity?.key)">{{ variant.label ?? variant.key }}</v-btn>
        </td><td /></tr>
      </template>
        </tbody>
      </table>
      </div>
    </template>
  </div>
  </ParametersSection>
</template>

<style scoped>
.custom-home-picker {
  display: grid;
  gap: 12px;
}

.custom-home-picker__table-wrap {
  overflow-x: auto;
}

.custom-home-picker__table {
  width: 100%;
  border-collapse: collapse;
}

.custom-home-picker__table th,
.custom-home-picker__table td {
  padding: 8px;
  border-bottom: 1px solid var(--border-color-primary);
  text-align: left;
}

.custom-home-picker__table th:nth-child(n + 3),
.custom-home-picker__table td:nth-child(n + 3) {
  width: 1%;
  text-align: right;
  white-space: nowrap;
}
</style>