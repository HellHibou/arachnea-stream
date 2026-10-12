<script setup lang="ts">
import { computed, defineAsyncComponent, shallowRef, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useCustomHomeManager } from '@/composables/home/useCustomHomeManager'
import { applyCustomHomeSelection } from '@/services/customHomeDraft'
import { createCategoryReference, createSectionReference } from '@/services/customHomeReferences'
import { loadHomeSourceDiscovery } from '@/services/rustify'
import { useI18n } from '@/i18n'
import { useStorage } from '@/services/storage'
import type { CustomHomeAddIntent, CustomHomeDefinition } from '@/types/customHome'
import { applyQuickAddSelections, resolveQuickAddSelections } from '@/services/customHomeQuickAdd'

const CustomHomeEditor = defineAsyncComponent(() => import('@/components/home/CustomHomeEditor.vue'))
const editor = shallowRef<{ save: () => void; canSave: boolean } | null>(null)

/** Optional add request or current source for an explicitly requested source-home draft. @default null */
const props = withDefaults(defineProps<{ intent?: CustomHomeAddIntent | null; sourceId?: string | null;
  open?: boolean; showTrigger?: boolean; editHomeId?: string | null; createOnOpen?: boolean }>(), {
  intent: null, sourceId: null, open: false, showTrigger: true, editHomeId: null, createOnOpen: false,
})
const emit = defineEmits<{ 'update:open': [open: boolean]; saved: [definition: CustomHomeDefinition] }>()
const { t } = useI18n()
const editingEnabled = useStorage().getHomePreferences().showSectionEditingButtons
const router = useRouter()
const route = useRoute()
const manager = useCustomHomeManager()
const visible = shallowRef(false)
const destination = shallowRef('')
const newHomeName = shallowRef('')
const categoryMode = shallowRef<'shortcut' | 'section'>('shortcut')
const deletion = shallowRef<CustomHomeDefinition | null>(null)
const seeding = shallowRef(false)
const session = shallowRef(0)
const choices = computed(() => [{ title: t('customHome.create'), value: '' },
  ...manager.homes.value.filter((home) => props.intent?.kind !== 'category' ||
    route.name !== 'custom-home' || home.id !== route.params.homeId).map((home) => ({ title: home.name, value: home.id }))])
watch(() => props.open, (value) => {
  if (value) {
    show()
    if (props.editHomeId || props.createOnOpen) void begin(props.editHomeId ?? '')
  }
})
watch(() => route.fullPath, () => { if (!manager.busy.value && !seeding.value) close() })

/** Opens management without fetching provider homes. */
function show(): void {
  visible.value = true; destination.value = ''; newHomeName.value = ''; categoryMode.value = 'shortcut'
  manager.cancel(); void manager.refresh()
}
/** Discards a draft; never persists implicit destination choices. */
function close(): void {
  if (manager.busy.value || seeding.value) return
  manager.cancel(); deletion.value = null; visible.value = false; emit('update:open', false)
}
/** @param id Explicit target, or empty for creation. */
async function begin(id = ''): Promise<void> {
  if (!id) await manager.refresh()
  const draft = await manager.begin(id || undefined)
  if (!draft) return
  if (!id) manager.draft.value = { ...draft, name: t('customHome.newName') }
  session.value++
}
/** Commits the clicked content once; no picker/editor step, navigation or write on cancel. */
async function add(): Promise<void> {
  if (!props.intent || seeding.value || manager.busy.value || (!destination.value && !newHomeName.value.trim())) return
  const intent = props.intent
  seeding.value = true
  try {
    const selections = await resolveQuickAddSelections(intent, categoryMode.value)
    if (!destination.value) await manager.refresh()
    const draft = await manager.begin(destination.value || undefined)
    if (!draft) return
    const value = applyQuickAddSelections(destination.value ? draft : { ...draft, name: newHomeName.value.trim() }, selections)
    const saved = await manager.save(value)
    if (saved) {
      emit('saved', saved)
      seeding.value = false
      close()
    }
  } catch (error) { manager.errorMessage.value = error instanceof Error ? error.message : String(error) }
  finally { seeding.value = false }
}
/** @param draft Editor snapshot; closes only after transaction success. */
async function save(draft: CustomHomeDefinition): Promise<void> {
  const saved = await manager.save(draft)
  if (!saved) return
  emit('saved', saved)
  close()
  await router.push({ name: 'custom-home', params: { homeId: saved.id } })
}
/** Deletes only the explicitly confirmed revision and returns the active deleted route to `/`. */
async function confirmDelete(): Promise<void> {
  const home = deletion.value
  if (!home || !await manager.remove(home)) return
  deletion.value = null
  if (route.name === 'custom-home' && route.params.homeId === home.id) {
    close(); await router.push({ name: 'home' })
  }
}
/** Copies a source discovery into a detached draft only after this explicit action. */
async function fromSource(): Promise<void> {
  if (!props.sourceId || seeding.value) return
  await begin()
  if (!manager.draft.value) return
  seeding.value = true
  try {
    const row = (await loadHomeSourceDiscovery([props.sourceId]))[0]
    if (!row?.available) throw new Error(`Source unavailable: ${props.sourceId}`)
    let draft = applyCustomHomeSelection(manager.draft.value, { kind: 'banners', sourceId: props.sourceId })
    for (const category of row.catalog.categories) draft = applyCustomHomeSelection(draft, { kind: 'shortcut', reference: createCategoryReference(category) })
    for (const section of row.catalog.sections) draft = applyCustomHomeSelection(draft, { kind: 'block', block: {
      id: crypto.randomUUID(), kind: 'home-section', reference: createSectionReference(section), titleOverride: null,
    } })
    manager.draft.value = draft
    session.value++
  } catch (error) { manager.errorMessage.value = error instanceof Error ? error.message : String(error) }
  finally { seeding.value = false }
}
</script>

<template>
  <v-btn v-if="props.showTrigger && editingEnabled" @click="show">{{ t('customHome.manage') }}</v-btn>
  <v-dialog class="custom-home-manager-overlay" :model-value="visible" :max-width="!props.intent && manager.draft.value ? 1000 : 680" :persistent="manager.busy.value || seeding" @update:model-value="(value) => { if (!value) close() }">
    <v-card class="custom-home-manager__dialog" :title="t(props.intent ? 'customHome.addToHome' : 'customHome.manage')">
      <v-card-text class="custom-home-manager__body">
        <v-alert v-if="manager.errorMessage.value && (props.intent || !manager.draft.value)" type="error" role="alert">
          {{ manager.errorMessage.value }}<v-btn @click="manager.refresh">{{ t('sourceHome.retry') }}</v-btn>
        </v-alert>
        <template v-if="props.intent">
          <v-select v-model="destination" :items="choices" :label="t('customHome.destination')" :disabled="manager.busy.value || seeding" />
          <v-text-field v-if="!destination" v-model="newHomeName" :label="t('customHome.name')" :disabled="manager.busy.value || seeding" />
          <v-radio-group v-if="props.intent.kind === 'category'" v-model="categoryMode" :label="t('customHome.categoryMode')"
            :disabled="manager.busy.value || seeding" inline>
            <v-radio :label="t('customHome.shortcut')" value="shortcut" />
            <v-radio :label="t('customHome.categorySection')" value="section" />
          </v-radio-group>
        </template>
        <template v-else-if="!manager.draft.value">
            <v-btn :disabled="manager.busy.value" @click="begin()">{{ t('customHome.create') }}</v-btn>
            <v-btn v-if="props.sourceId" :disabled="manager.busy.value || seeding" @click="fromSource">{{ t('customHome.fromSource') }}</v-btn>
            <div v-for="(home, index) in manager.homes.value" :key="home.id" class="custom-home-manager__row">
              <RouterLink :to="{ name: 'custom-home', params: { homeId: home.id } }">{{ home.name }}</RouterLink>
              <v-btn :disabled="manager.busy.value" @click="begin(home.id)">{{ t('customHome.edit') }}</v-btn>
              <v-btn :disabled="manager.busy.value || index === 0" @click="manager.move(index, -1)">{{ t('customHome.moveUp') }}</v-btn>
              <v-btn :disabled="manager.busy.value || index === manager.homes.value.length - 1" @click="manager.move(index, 1)">{{ t('customHome.moveDown') }}</v-btn>
              <v-btn :disabled="manager.busy.value" @click="deletion = home">{{ t('customHome.delete') }}</v-btn>
            </div>
        </template>
        <template v-else-if="manager.draft.value">
          <CustomHomeEditor ref="editor" :key="session" :definition="manager.draft.value" :busy="manager.busy.value || seeding"
            :error-message="manager.errorMessage.value" @save="save" @cancel="close" />
        </template>
      </v-card-text>
      <v-card-actions v-if="!props.intent && manager.draft.value" class="custom-home-manager__footer">
        <button class="custom-home-manager__action" type="button" :disabled="!editor?.canSave || manager.busy.value || seeding" @click="editor?.save()">{{ t('customHome.save') }}</button>
        <button class="custom-home-manager__action" type="button" :disabled="manager.busy.value || seeding" @click="close">{{ t('customHome.cancel') }}</button>
      </v-card-actions>
      <v-card-actions v-else>
        <v-btn :disabled="manager.busy.value || seeding" @click="close">{{ t(props.intent ? 'customHome.cancel' : 'customHome.close') }}</v-btn>
        <v-btn v-if="props.intent" :loading="manager.busy.value || seeding"
          :disabled="manager.busy.value || seeding || (!destination && !newHomeName.trim())" @click="add">{{ t('customHome.add') }}</v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
  <v-dialog :model-value="Boolean(deletion)" max-width="500" :persistent="manager.busy.value" @update:model-value="(value) => { if (!value) deletion = null }">
    <v-card :title="t('customHome.confirmDelete')">
      <v-card-text>{{ deletion?.name }}</v-card-text>
      <v-card-actions>
        <v-btn :disabled="manager.busy.value" @click="deletion = null">{{ t('customHome.cancel') }}</v-btn>
        <v-btn :loading="manager.busy.value" @click="confirmDelete">{{ t('customHome.delete') }}</v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
:global(.custom-home-manager-overlay .v-overlay__scrim) {
  background: var(--bg-transparent);
  opacity: 1;
  backdrop-filter: var(--backdrop-filter-strong);
  -webkit-backdrop-filter: var(--backdrop-filter-strong);
}

.custom-home-manager__dialog {
  display: flex;
  flex-direction: column;
  max-height: calc(100dvh - 48px);
  overflow: hidden;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
  box-shadow: var(--inset-light);
}

.custom-home-manager__body {
  min-height: 0;
  overflow-y: auto;
}

.custom-home-manager__dialog :deep(.v-card-item),
.custom-home-manager__dialog :deep(.v-card-actions) {
  flex-shrink: 0;
}

.custom-home-manager__action {
  min-height: var(--control-height);
  padding: 0 20px;
  border-radius: var(--radius);
  background: var(--bg-accent-primary);
  color: var(--text-primary);
  font: inherit;
  font-weight: 700;
  cursor: pointer;
}

.custom-home-manager__action:disabled {
  opacity: 0.42;
  cursor: default;
}

.custom-home-manager__action:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.custom-home-manager__dialog :deep(.v-card-title) {
  font-size: 1.16rem;
  font-weight: 800;
}

.custom-home-manager__dialog :deep(.v-btn) {
  border: 1px solid var(--border-color-primary);
  background: var(--bg-transparent);
  color: var(--text-primary);
  font: inherit;
  text-transform: none;
}

.custom-home-manager__dialog :deep(.v-field) {
  background: var(--bg-transparent);
  color: var(--text-primary);
  font: inherit;
}
.custom-home-manager__row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  margin-block: 12px;
}
</style>