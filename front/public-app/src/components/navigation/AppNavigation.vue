<script setup lang="ts">
import { computed, nextTick, onMounted, onScopeDispose, shallowRef, useTemplateRef, watch } from 'vue'
import { useRoute } from 'vue-router'
import AppSidebar from '@/components/navigation/AppSidebar.vue'
import CustomHomeManager from '@/components/home/CustomHomeManager.vue'
import { useCustomHomeManager } from '@/composables/home/useCustomHomeManager'
import { useServiceMetadata } from '@/composables/useServiceMetadata'
import { useI18n } from '@/i18n'
import { useStorage } from '@/services/storage'

/** Shell-owned responsive state; this component owns navigation records, not content resolution. */
const props = defineProps<{ mobile: boolean; open: boolean; hidden: boolean }>()
const emit = defineEmits<{ close: []; settings: []; 'update:width': [width: number] }>()
const route = useRoute()
const { t } = useI18n()
const manager = useCustomHomeManager()
const metadata = useServiceMetadata()
const editingEnabled = useStorage().getHomePreferences().showSectionEditingButtons
const surface = useTemplateRef<HTMLElement>('surface')
let widthObserver: ResizeObserver | null = null
watch(surface, (element) => {
  widthObserver?.disconnect()
  if (!element) return
  widthObserver = new ResizeObserver(() => {
    if (!props.mobile) emit('update:width', Math.ceil(element.getBoundingClientRect().width))
  })
  widthObserver.observe(element)
}, { flush: 'post' })
const managerOpen = shallowRef(false)
const editorTarget = shallowRef<string | null>(null)
const createRequested = shallowRef(false)
const preferenceError = shallowRef<string | null>(null)
const expansionKey = 'arachnea.navigation.sourcesExpanded'
const sourcesExpanded = shallowRef(true)
const sources = computed(() => metadata.services.value.filter((source) => source.supportedQueries.includes('load_home')))
const activeHomeId = computed(() => route.name === 'custom-home' ? String(route.meta.homeId) : null)
const activeSourceId = computed(() => route.name === 'source-home' ? String(route.params.sourceId) : null)

/** @param expanded Group state; unavailable storage is reported without blocking navigation. */
function setExpanded(expanded: boolean): void {
  sourcesExpanded.value = expanded
  try { localStorage.setItem(expansionKey, JSON.stringify(expanded)); preferenceError.value = null }
  catch { preferenceError.value = t('navigation.storageError') }
}
/** @param id Existing home id; null opens management, creation is an explicit separate request. @param create Creation intent. */
function openManager(id: string | null = null, create = false): void {
  editorTarget.value = id
  createRequested.value = create
  if (surface.value instanceof HTMLDialogElement && surface.value.open) surface.value.close()
  emit('close')
  void nextTick(() => { managerOpen.value = true })
}
/** Reflects mobile shell state into the native modal, preserving its keyboard/focus trap. */
function syncDialog(): void {
  const dialog = surface.value
  if (!(dialog instanceof HTMLDialogElement)) return
  if (props.open && !props.hidden && !dialog.open) dialog.showModal()
  else if ((!props.open || props.hidden) && dialog.open) dialog.close()
}
/** @param event Native backdrop click; content clicks never dismiss navigation. */
function dismissBackdrop(event: MouseEvent): void {
  if (!props.mobile || !surface.value || event.target !== surface.value) return
  const rect = surface.value.getBoundingClientRect()
  if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) emit('close')
}
watch(() => [props.mobile, props.open, props.hidden], () => { void syncDialog() }, { flush: 'post' })
watch(() => route.fullPath, () => emit('close'))
onMounted(() => {
  readExpansion()
  void manager.refresh()
  void metadata.load()
  void syncDialog()
})
/** @param event Cross-tab preference updates; native independent views reread on focus. */
function onStorage(event: StorageEvent): void {
  if (event.key === expansionKey || event.key === null) readExpansion()
}
/** Reads the current persistent preference without resetting it when storage is unavailable. */
function readExpansion(): void {
  try { sourcesExpanded.value = localStorage.getItem(expansionKey) !== 'false'; preferenceError.value = null }
  catch { preferenceError.value = t('navigation.storageError') }
}
/** Refreshes expansion after a suspended native view or browser tab becomes visible. */
function onVisibility(): void { if (document.visibilityState === 'visible') readExpansion() }
window.addEventListener('storage', onStorage)
window.addEventListener('focus', readExpansion)
document.addEventListener('visibilitychange', onVisibility)
onScopeDispose(() => {
  widthObserver?.disconnect()
  window.removeEventListener('storage', onStorage)
  window.removeEventListener('focus', readExpansion)
  document.removeEventListener('visibilitychange', onVisibility)
  const dialog = surface.value
  if (dialog instanceof HTMLDialogElement && dialog.open) dialog.close()
})
</script>

<template>
  <component :is="props.mobile ? 'dialog' : 'aside'" v-if="!props.hidden" id="app-navigation" ref="surface"
    class="app-navigation" :class="{ 'app-navigation--mobile': props.mobile }" :aria-label="t('navigation.label')"
    @cancel.prevent="emit('close')" @click="dismissBackdrop">
    <button v-if="props.mobile" type="button" class="app-navigation__close" @click="emit('close')">
      {{ t('navigation.close') }} <v-icon icon="mdi-close" aria-hidden="true" />
    </button>
    <AppSidebar @settings="emit('close'); emit('settings')" :homes="manager.homes.value" :sources="sources" :sources-expanded="sourcesExpanded"
      :editing-enabled="editingEnabled"
      :active-home-id="activeHomeId" :active-source-id="activeSourceId" :lives-active="route.name === 'lives' || route.name === 'live-details'"
      @navigate="emit('close')" @create="openManager(null, true)" @manage="openManager()" @edit="openManager($event)"
      @update:sources-expanded="setExpanded" />
    <p v-if="metadata.isLoading.value" class="app-navigation__status" role="status">{{ t('entry.loadingMessage') }}</p>
    <div v-if="manager.errorMessage.value || metadata.errorMessage.value || preferenceError" class="app-navigation__status" role="status">
      <p>{{ manager.errorMessage.value ?? metadata.errorMessage.value ?? preferenceError }}</p>
      <v-btn @click="manager.refresh(); metadata.refresh()">{{ t('sourceHome.retry') }}</v-btn>
    </div>
  </component>
  <CustomHomeManager v-model:open="managerOpen" :show-trigger="false" :edit-home-id="editorTarget"
    :source-id="activeSourceId" :create-on-open="createRequested" />
</template>

<style scoped>
.app-navigation {
  position: fixed;
  top: var(--app-toolbar-height, 64px);
  bottom: 0;
  left: 0;
  z-index: 5;
  width: max-content;
  min-width: 220px;
  overflow: auto;
  overscroll-behavior: contain;
  background: var(--bg-surface);
  border-right: 1px solid var(--border-color-primary);
}

.app-navigation--mobile {
  top: 0;
  right: auto;
  width: max-content;
  min-width: min(220px, 90vw);
  max-width: 90vw;
  height: 100dvh;
  max-height: none;
  margin: 0;
  padding: 0;
  border: 0;
  color: var(--text-primary);
}

.app-navigation--mobile::backdrop {
  background: rgb(0 0 0 / 60%);
}

.app-navigation__close {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  min-height: 44px;
  padding: 12px;
  border: 0;
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
  cursor: pointer;
}

.app-navigation__close:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: -2px;
}

.app-navigation__status {
  padding: 12px;
  overflow-wrap: anywhere;
}
</style>