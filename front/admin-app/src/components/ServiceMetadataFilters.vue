<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from '@/i18n'
import { formatLanguageLabel } from '@/services/languages'
import type { ServiceFilterDefinition } from '@/services/appConfig'
import type { AdminServiceEntry } from '@/services/adminApi'

const props = defineProps<{ definitions: readonly ServiceFilterDefinition[]; services: AdminServiceEntry[] }>()
const query = defineModel<string>('query', { required: true })
const selected = defineModel<Record<string, string[]>>('selected', { required: true })
const { t } = useI18n()
const filters = computed(() => props.definitions.map((definition) => ({
  key: definition.key,
  label: t(definition.label),
  options: [...new Set(props.services.flatMap((service) => service.metadata?.[definition.key] ?? []))].sort()
    .map((value) => ({
      value,
      title: definition.value_labels?.[value]
        ? t(definition.value_labels[value])
        : definition.value_format === 'language'
          ? formatLanguageLabel(value)
          : value,
    })),
})))
function update(key: string, values: string[]) {
  selected.value = { ...selected.value, [key]: values }
}
</script>

<template>
  <v-row class="mb-2">
    <v-col cols="12" md="4">
      <v-text-field v-model="query" :label="t('services.search')" prepend-inner-icon="mdi-magnify"
        clearable @click:clear="query = ''" hide-details />
    </v-col>
    <v-col v-for="filter in filters" :key="filter.key" cols="12" md="2">
      <v-select :model-value="selected[filter.key] ?? []" :label="filter.label" :items="filter.options" item-title="title" item-value="value"
        multiple chips closable-chips clearable hide-details
        @update:model-value="update(filter.key, $event ?? [])" />
    </v-col>
  </v-row>
</template>
