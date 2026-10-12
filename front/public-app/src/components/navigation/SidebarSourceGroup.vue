<script setup lang="ts">
import type { ServiceMetadata } from '@/types/serviceMetadata'
import { useI18n } from '@/i18n'

/** Capability-filtered source entries and controlled persistent expansion state. */
defineProps<{ sources: readonly Pick<ServiceMetadata, 'id' | 'title' | 'logo'>[]; expanded: boolean; activeSourceId: string | null; groupId: string }>()
const emit = defineEmits<{ 'update:expanded': [expanded: boolean]; navigate: [] }>()
const { t } = useI18n()
</script>

<template>
  <section>
    <button class="sidebar-link" type="button" :aria-expanded="expanded" :aria-controls="groupId"
      @click="emit('update:expanded', !expanded)">
      <v-icon icon="mdi-projector" aria-hidden="true" />
      <strong>{{ t('navigation.sources') }}</strong>
    </button>
    <ul v-show="expanded" :id="groupId" class="sidebar-list sidebar-services">
      <li v-for="source in sources" :key="source.id">
        <RouterLink class="sidebar-link" :to="{ name: 'source-home', params: { sourceId: source.id } }"
          :aria-current="activeSourceId === source.id ? 'page' : undefined" :title="source.title" @click="emit('navigate')">
          <img v-if="source.logo" class="sidebar-logo" :src="source.logo" alt="" loading="lazy" @error="($event.target as HTMLImageElement).hidden = true">
          <v-icon v-else icon="mdi-television" aria-hidden="true" />
          <span class="sidebar-label">{{ source.title }}</span>
        </RouterLink>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.sidebar-services {
  margin-left: 14px;
}
</style>