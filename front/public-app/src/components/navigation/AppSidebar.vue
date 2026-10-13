<script setup lang="ts">
import { customHomeRoute } from '@/services/customHomeRoute'
import { useId } from 'vue'
import SidebarSourceGroup from '@/components/navigation/SidebarSourceGroup.vue'
import { useI18n } from '@/i18n'
import type { CustomHomeDefinition } from '@/types/customHome'
import type { ServiceMetadata } from '@/types/serviceMetadata'

/** Presentation-only destinations; editing and navigation close requests are emitted upward. */
defineProps<{ homes: CustomHomeDefinition[]; sources: readonly Pick<ServiceMetadata, 'id' | 'title' | 'logo'>[]; sourcesExpanded: boolean;
  activeHomeId: string | null; activeSourceId: string | null; livesActive: boolean; editingEnabled: boolean }>()
const emit = defineEmits<{ settings: []; navigate: []; create: []; manage: []; edit: [id: string];
  'update:sourcesExpanded': [expanded: boolean] }>()
const sourceGroupId = useId()
const { t } = useI18n()
</script>

<template>
  <nav :aria-label="t('navigation.label')" class="app-sidebar">
    <section :aria-label="t('navigation.homes')">
      <div class="app-sidebar__heading-row">
        <RouterLink class="sidebar-link" :to="{ name: 'home' }" @click="emit('navigate')">
          <v-icon icon="mdi-home-outline" size="20" aria-hidden="true" /><strong>{{ t('navigation.homes') }}</strong>
        </RouterLink>
        <button v-if="editingEnabled" class="app-sidebar__edit" type="button" :aria-label="t('customHome.manage')" :title="t('customHome.manage')"
          @click="emit('manage')"><v-icon icon="mdi-pencil-outline" size="18" aria-hidden="true" /></button>
      </div>
      <ul class="sidebar-list sidebar-services">
        <li v-for="home in homes" :key="home.id" class="app-sidebar__home">
          <RouterLink class="sidebar-link app-sidebar__home-link" :title="home.name"
            :aria-current="activeHomeId === home.id ? 'page' : undefined"
            :to="customHomeRoute(home)" @click="emit('navigate')">
            <span class="sidebar-label">{{ home.name }}</span>
          </RouterLink>
          <button v-if="editingEnabled" class="app-sidebar__edit" type="button" :aria-label="`${t('customHome.edit')}: ${home.name}`"
            :title="`${t('customHome.edit')}: ${home.name}`" @click="emit('edit', home.id)">
            <v-icon icon="mdi-pencil-outline" size="18" aria-hidden="true" />
          </button>
        </li>
      </ul>
    </section>
    <RouterLink class="sidebar-link" :to="{ name: 'lives' }" :aria-current="livesActive ? 'page' : undefined" @click="emit('navigate')">
      <v-icon icon="mdi-television-play" aria-hidden="true" /><strong>{{ t('toolbar.lives') }}</strong>
    </RouterLink>
    <SidebarSourceGroup :sources="sources" :expanded="sourcesExpanded" :active-source-id="activeSourceId"
      :group-id="sourceGroupId" @update:expanded="emit('update:sourcesExpanded', $event)" @navigate="emit('navigate')" />
    <button class="sidebar-link" type="button" @click="emit('settings')">
      <v-icon icon="mdi-cog" aria-hidden="true" /><strong>{{ t('navigation.settings') }}</strong>
    </button>
  </nav>
</template>

<style scoped>
.app-sidebar :deep(.sidebar-link[aria-current='page']) {
  font-weight: 700;
}

.app-sidebar__create {
  justify-content: space-between;
}

.app-sidebar__create-icon {
  width: 20px;
  flex-shrink: 0;
}
.app-sidebar {
  display: grid;
  gap: 4px;
  padding: 12px;
  color: var(--text-primary);
}

.app-sidebar__heading {
  margin: 4px 8px;
  font-size: 1rem;
}

.app-sidebar__heading-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.app-sidebar :deep(.sidebar-list) {
  margin: 0;
  padding: 0;
  list-style: none;
}

.app-sidebar :deep(.sidebar-link),
.app-sidebar__edit {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  min-height: 36px;
  padding: 4px 8px;
  border: 0;
  border-radius: var(--radius);
  background: transparent;
  color: var(--text-primary);
  font: inherit;
  text-align: start;
  text-decoration: none;
  cursor: pointer;
  white-space: nowrap;
}

.app-sidebar :deep(.sidebar-link:hover),
.app-sidebar :deep(.sidebar-link[aria-current='page']),
.app-sidebar__edit:hover {
  background: var(--bg-surface);
  color: var(--color-primary);
}

.app-sidebar :deep(.sidebar-link:focus-visible),
.app-sidebar__edit:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: -2px;
}

.app-sidebar :deep(.sidebar-label) {
  min-width: 0;
  white-space: nowrap;
}

.app-sidebar :deep(.sidebar-logo) {
  width: 24px;
  height: 24px;
  object-fit: contain;
}

.app-sidebar__home {
  display: flex;
  align-items: center;
}

.app-sidebar :deep(.sidebar-services) {
  margin-left: 14px;
}

.app-sidebar__home-link {
  min-width: 0;
  flex: 1;
}

.app-sidebar__edit {
  width: 36px;
  flex-shrink: 0;
}
</style>