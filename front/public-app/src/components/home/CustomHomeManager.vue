<script setup lang="ts">
import { customHomeRoute } from '@/services/customHomeRoute'
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
import { createUuid } from '@/utils'
import DefaultHomeSetting from './DefaultHomeSetting.vue'

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
const defaultHomeId = useStorage().getParameters().defaultHomeId
const router = useRouter()
const route = useRoute()
const manager = useCustomHomeManager()
const visible = shallowRef(false)
const destination = shallowRef('')
const newHomeName = shallowRef('')
const categoryMode = shallowRef<'shortcut' | 'section'>('shortcut')
const selectedCategoryIds = shallowRef<string[]>([])
const deletion = shallowRef<CustomHomeDefinition | null>(null)
const seeding = shallowRef(false)
const session = shallowRef(0)
const editingFromList = shallowRef(false)
const directEditor = shallowRef(false)
const editorSnapshot = shallowRef<CustomHomeDefinition | null>(null)
const editorDefinition = computed(() => manager.draft.value ?? editorSnapshot.value)
const choices = computed(() => [{ title: t('customHome.create'), value: '' },
  ...manager.homes.value.filter((home) => props.intent?.kind !== 'category' ||
    route.name !== 'custom-home' || home.id !== route.meta.homeId).map((home) => ({ title: home.name, value: home.id }))])
watch(() => props.open, (value) => {
  if (value) {
    show()
    if (props.editHomeId || props.createOnOpen) void begin(props.editHomeId ?? '')
  }
})
watch(() => route.fullPath, () => { if (!manager.busy.value && !seeding.value) close() })

/** Opens management without fetching provider homes. */
function show(): void {
  directEditor.value = Boolean(props.editHomeId || props.createOnOpen)
  editorSnapshot.value = null
  visible.value = true; destination.value = ''; newHomeName.value = ''; categoryMode.value = 'shortcut'
  selectedCategoryIds.value = []
  manager.cancel(); void manager.refresh()
}
/** Discards a draft; never persists implicit destination choices. */
function close(): void {
  if (manager.busy.value || seeding.value) return
  manager.cancel(); editingFromList.value = false; deletion.value = null; visible.value = false; emit('update:open', false)
}
/** @param id Explicit target, or empty for creation. @param fromList Whether completion returns to management. */
async function begin(id = '', fromList = false): Promise<void> {
  if (!id) await manager.refresh()
  const draft = await manager.begin(id || undefined)
  if (!draft) return
  editorSnapshot.value = draft
  editingFromList.value = fromList
  if (!id) manager.draft.value = { ...draft, name: t('customHome.newName') }
  session.value++
}
/** Discards only the list-opened editor, keeping its management dialog and route intact. */
function cancelListEditor(): void {
  if (manager.busy.value || seeding.value) return
  manager.cancel()
  editingFromList.value = false
}
/** Commits the clicked content once; no picker/editor step, navigation or write on cancel. */
async function add(): Promise<void> {
  if (!props.intent || seeding.value || manager.busy.value || (!destination.value && !newHomeName.value.trim())) return
  const intent = props.intent.kind === 'categories' ? { ...props.intent,
    categories: props.intent.categories.filter((category) => selectedCategoryIds.value.includes(category.id)) } : props.intent
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
  const returnToList = editingFromList.value
  const saved = await manager.save(draft)
  if (!saved) return
  emit('saved', saved)
  if (route.name === 'custom-home' && route.meta.homeId === saved.id) await router.replace(customHomeRoute(saved))
  if (returnToList) { editingFromList.value = false; return }
  close()
  await router.push(customHomeRoute(saved))
}
/** Deletes only the explicitly confirmed revision and returns the active deleted route to `/`. */
async function confirmDelete(): Promise<void> {
  const home = deletion.value
  if (!home || !await manager.remove(home)) return
  deletion.value = null
  if (route.name === 'custom-home' && route.meta.homeId === home.id) {
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
      id: createUuid(), kind: 'home-section', reference: createSectionReference(section), titleOverride: null,
    } })
    manager.draft.value = draft
    session.value++
  } catch (error) { manager.errorMessage.value = error instanceof Error ? error.message : String(error) }
  finally { seeding.value = false }
}
</script>

<template>
  <v-btn v-if="props.showTrigger && editingEnabled" @click="show">{{ t('customHome.manage') }}</v-btn>
  <v-dialog class="custom-home-manager-overlay" :model-value="visible" :max-width="!props.intent && (manager.draft.value || directEditor) && !editingFromList ? 1280 : 680" :persistent="manager.busy.value || seeding || editingFromList" @update:model-value="(value) => { if (!value) close() }">
    <v-card class="custom-home-manager__dialog" :title="t(props.intent ? 'customHome.addToHome' : 'customHome.manage')">
      <v-card-text class="custom-home-manager__body">
        <v-alert v-if="manager.errorMessage.value && (props.intent || !manager.draft.value)" type="error" role="alert">
          {{ manager.errorMessage.value }}<v-btn @click="manager.refresh">{{ t('sourceHome.retry') }}</v-btn>
        </v-alert>
        <template v-if="props.intent">
          <v-select v-model="destination" :items="choices" :label="t('customHome.destination')" :disabled="manager.busy.value || seeding" />
          <v-text-field v-if="!destination" v-model="newHomeName" :label="t('customHome.name')" :disabled="manager.busy.value || seeding" />
          <div v-if="props.intent.kind === 'categories'" class="custom-home-manager__category-choices">
            <v-checkbox v-for="category in props.intent.categories" :key="category.id" v-model="selectedCategoryIds"
              :value="category.id" :label="category.label" :disabled="manager.busy.value || seeding" density="compact" hide-details />
          </div>
          <v-radio-group v-if="props.intent.kind === 'category' || props.intent.kind === 'categories'" v-model="categoryMode" :label="t('customHome.categoryMode')"
            :disabled="manager.busy.value || seeding" inline>
            <v-radio :label="t('customHome.shortcut')" value="shortcut" />
            <v-radio :label="t('customHome.categorySection')" value="section" />
          </v-radio-group>
        </template>
        <template v-else-if="(!manager.draft.value && !directEditor) || editingFromList">
            <DefaultHomeSetting />
            <div class="custom-home-manager__table-scroll">
              <table class="custom-home-manager__table" :aria-label="t('customHome.manage')">
                <tbody><tr v-for="(home, index) in manager.homes.value" :key="home.id">
                  <td><input class="custom-home-manager__checkbox" type="checkbox" :checked="defaultHomeId === home.id"
                    :disabled="manager.busy.value || seeding" :aria-label="`${t('customHome.isDefault')}: ${home.name}`"
                    :title="t('customHome.setDefault')"
                    @change="defaultHomeId = ($event.target as HTMLInputElement).checked ? home.id : null"></td>
                  <td>{{ home.name }}</td>
                  <td><div class="custom-home-manager__tools">
                    <button class="custom-home-manager__icon" type="button" :disabled="manager.busy.value"
                      :aria-label="t('customHome.edit')" :title="t('customHome.edit')" @click="begin(home.id, true)">
                      <v-icon icon="mdi-pencil-outline" size="18" aria-hidden="true" />
                    </button>
                    <button class="custom-home-manager__icon" type="button" :disabled="manager.busy.value || index === 0"
                      :aria-label="t('customHome.moveUp')" :title="t('customHome.moveUp')" @click="manager.move(index, -1)">
                      <v-icon icon="mdi-arrow-up" size="18" aria-hidden="true" />
                    </button>
                    <button class="custom-home-manager__icon" type="button" :disabled="manager.busy.value || index === manager.homes.value.length - 1"
                      :aria-label="t('customHome.moveDown')" :title="t('customHome.moveDown')" @click="manager.move(index, 1)">
                      <v-icon icon="mdi-arrow-down" size="18" aria-hidden="true" />
                    </button>
                    <button class="custom-home-manager__icon" type="button" :disabled="manager.busy.value"
                      :aria-label="t('customHome.delete')" :title="t('customHome.delete')" @click="deletion = home">
                      <v-icon icon="mdi-trash-can-outline" size="18" aria-hidden="true" />
                    </button>
                  </div></td>
                </tr>
                <tr><td colspan="3" class="custom-home-manager__creation"><div class="custom-home-manager__tools">
                  <button class="custom-home-manager__icon" type="button" :disabled="manager.busy.value"
                    :aria-label="t('customHome.create')" :title="t('customHome.create')" @click="begin('', true)">
                    <v-icon icon="mdi-plus" size="18" aria-hidden="true" />
                  </button>
                  <button v-if="props.sourceId" class="custom-home-manager__icon" type="button" :disabled="manager.busy.value || seeding"
                    :aria-label="t('customHome.fromSource')" :title="t('customHome.fromSource')" @click="fromSource">
                    <v-icon icon="mdi-content-copy" size="18" aria-hidden="true" />
                  </button>
                </div></td></tr></tbody>
              </table>
            </div>
        </template>
        <template v-else-if="editorDefinition">
          <CustomHomeEditor ref="editor" :key="session" :definition="editorDefinition" :busy="manager.busy.value || seeding"
            :error-message="manager.errorMessage.value" @save="save" @cancel="close" />
        </template>
      </v-card-text>
      <v-card-actions v-if="!props.intent && (manager.draft.value || directEditor) && !editingFromList" class="custom-home-manager__footer">
        <button class="custom-home-manager__action" type="button" :disabled="!editor?.canSave || manager.busy.value || seeding" @click="editor?.save()">{{ t('customHome.save') }}</button>
        <button class="custom-home-manager__action" type="button" :disabled="manager.busy.value || seeding" @click="close">{{ t('customHome.cancel') }}</button>
      </v-card-actions>
      <v-card-actions v-else>
        <button class="custom-home-manager__action" type="button" :disabled="manager.busy.value || seeding" @click="close">{{ t(props.intent ? 'customHome.cancel' : 'customHome.close') }}</button>
        <button v-if="props.intent" class="custom-home-manager__action" type="button" :aria-busy="manager.busy.value || seeding"
          :disabled="manager.busy.value || seeding || (!destination && !newHomeName.trim()) || (props.intent.kind === 'categories' && !selectedCategoryIds.length)" @click="add">
          <v-progress-circular v-if="manager.busy.value || seeding" indeterminate size="18" width="2" aria-hidden="true" />
          {{ t('customHome.add') }}
        </button>
      </v-card-actions>
    </v-card>
  </v-dialog>
  <v-dialog class="custom-home-manager-overlay" :model-value="editingFromList" max-width="1280"
    :persistent="manager.busy.value || seeding" @update:model-value="(value) => { if (!value) cancelListEditor() }">
    <v-card class="custom-home-manager__dialog" :title="t('customHome.manage')">
      <v-card-text class="custom-home-manager__body">
        <CustomHomeEditor v-if="editingFromList && manager.draft.value" ref="editor" :key="session"
          :definition="manager.draft.value" :busy="manager.busy.value || seeding"
          :error-message="manager.errorMessage.value" @save="save" @cancel="cancelListEditor" />
      </v-card-text>
      <v-card-actions class="custom-home-manager__footer">
        <button class="custom-home-manager__action" type="button" :disabled="!editor?.canSave || manager.busy.value || seeding"
          @click="editor?.save()">{{ t('customHome.save') }}</button>
        <button class="custom-home-manager__action" type="button" :disabled="manager.busy.value || seeding"
          @click="cancelListEditor">{{ t('customHome.cancel') }}</button>
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
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-height: 40px;
  padding: 0 16px;
  border: 0;
  border-radius: 999px;
  background: var(--bg-accent-primary);
  box-shadow: 0 10px 18px var(--shadow-color);
  color: var(--text-primary);
  font: inherit;
  font-size: 0.96rem;
  font-weight: 700;
  cursor: pointer;
  transition:
    transform var(--duration-fast) ease,
    filter var(--duration-fast) ease;
}

.custom-home-manager__category-choices :deep(.v-selection-control) {
  min-height: 32px;
}

.custom-home-manager__category-choices :deep(.v-input) {
  margin: 0;
}

.custom-home-manager__action:hover:not(:disabled) {
  transform: translateY(-1px);
  filter: brightness(1.08);
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
.custom-home-manager__tools {
  display: inline-flex;
  gap: 6px;
  align-items: center;
}

.custom-home-manager__table-scroll {
  overflow-x: auto;
  margin-top: 12px;
}

.custom-home-manager__table {
  width: 100%;
  border-collapse: collapse;
}

.custom-home-manager__table th,
.custom-home-manager__table td {
  padding: 8px;
  border-bottom: 1px solid var(--border-color-primary);
  text-align: left;
  vertical-align: middle;
}

.custom-home-manager__table th:first-child,
.custom-home-manager__table td:first-child {
  width: 1%;
  text-align: center;
}

.custom-home-manager__table th:last-child,
.custom-home-manager__table td:last-child {
  width: 1%;
  text-align: right;
  white-space: nowrap;
}

.custom-home-manager__table td.custom-home-manager__creation {
  text-align: left;
}

.custom-home-manager__checkbox {
  width: 18px;
  height: 18px;
  accent-color: var(--color-primary);
  cursor: pointer;
}

.custom-home-manager__icon {
  display: inline-grid;
  place-items: center;
  width: 36px;
  height: 36px;
  border: 1px solid var(--border-color-primary);
  border-radius: 999px;
  background: var(--bg-transparent);
  color: var(--text-secondary);
  box-shadow: var(--inset-light);
  cursor: pointer;
}

.custom-home-manager__icon:hover:not(:disabled) {
  border-color: var(--color-primary);
  color: var(--text-primary);
}

.custom-home-manager__icon:disabled {
  opacity: 0.42;
  cursor: not-allowed;
}

.custom-home-manager__icon:focus-visible,
.custom-home-manager__checkbox:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>