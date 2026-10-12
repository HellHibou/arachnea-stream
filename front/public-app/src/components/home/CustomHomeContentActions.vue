<script setup lang="ts">
import { computed, shallowRef, watch } from 'vue'
import { saveCustomHome } from '@/services/customHomes'
import { mutateCustomHomeContent } from '@/services/customHomeContentMutation'
import type { CustomHomeDefinition } from '@/types/customHome'
import type { HomeCategory, HomeSection } from '@/types/home'
import { useI18n } from '@/i18n'

/** Explicit toolbar request; a merged section requires selecting a single owning block. */
export type CustomHomeContentRequest =
  | { kind: 'banners'; action: 'remove' }
  | { kind: 'section'; section: HomeSection; action: 'remove' | 'up' | 'down' }
  | { kind: 'category'; category: HomeCategory; action: 'remove' }

/** Persisted snapshot and requested operation. Null request hides the dialog. */
const props = defineProps<{ definition: CustomHomeDefinition; request: CustomHomeContentRequest | null }>()
const emit = defineEmits<{ close: []; saved: [definition: CustomHomeDefinition] }>()
const { t } = useI18n()
const snapshot = shallowRef<CustomHomeDefinition | null>(null)
const currentRequest = shallowRef<CustomHomeContentRequest | null>(null)
const blockId = shallowRef('')
const bannerSources = shallowRef<string[]>([])
const busy = shallowRef(false)
const errorMessage = shallowRef<string | null>(null)
const blocks = computed(() => {
  const request = currentRequest.value
  return request?.kind === 'section' ? (snapshot.value?.blocks ?? [])
    .filter((block) => request.section.customBlockIds?.includes(block.id)) : []
})
const selectedBlock = computed(() => blocks.value.find((block) => block.id === blockId.value))
const choices = computed(() => blocks.value.map((block) => ({ value: block.id,
  title: `${block.reference.sourceId} — ${block.titleOverride ?? block.reference.labelSnapshot ?? t('customHome.untitled')} (${(snapshot.value?.blocks.findIndex((item) => item.id === block.id) ?? -1) + 1})` })))
const canApply = computed(() => {
  const request = currentRequest.value
  const definition = snapshot.value
  if (!request || !definition || busy.value) return false
  if (request.kind === 'banners') return bannerSources.value.length > 0 && bannerSources.value.every((source) => definition.bannerSources.includes(source))
  if (request.kind === 'category') return definition.categoryShortcuts[request.category.customShortcutIndex ?? -1] !== undefined
  const index = definition.blocks.findIndex((block) => block.id === blockId.value)
  return index >= 0 && (request.action === 'remove' ||
    (request.action === 'up' ? index > 0 : index < definition.blocks.length - 1))
})
watch(() => props.request, (request) => {
  snapshot.value = props.definition
  currentRequest.value = request
  errorMessage.value = null
  blockId.value = blocks.value.length === 1 ? blocks.value[0]!.id : ''
  bannerSources.value = []
  if (request?.kind === 'section' && request.action !== 'remove' && blocks.value.length === 1) void apply()
}, { immediate: true })

/** Commits only the displayed snapshot revision; errors leave the dialog open without optimistic removal. */
async function apply(): Promise<void> {
  if (!canApply.value || !snapshot.value || !currentRequest.value) return
  const definition = snapshot.value
  const request = currentRequest.value
  busy.value = true
  errorMessage.value = null
  try {
    const next = request.kind === 'banners' ? { ...definition, bannerSources: definition.bannerSources.filter((source) => !bannerSources.value.includes(source)) }
      : mutateCustomHomeContent(definition, request.kind === 'category'
      ? { kind: 'shortcut', index: request.category.customShortcutIndex!, action: 'remove' }
      : { kind: 'block', blockId: blockId.value, action: request.action })
    const saved = await saveCustomHome(next, definition.updatedAt)
    window.dispatchEvent(new Event('custom-homes-changed'))
    emit('saved', saved)
    emit('close')
  } catch (error) { errorMessage.value = error instanceof Error ? error.message : String(error) }
  finally { busy.value = false }
}
</script>

<template>
  <v-dialog :model-value="Boolean(props.request)" max-width="600" :persistent="busy"
    @update:model-value="(open) => { if (!open && !busy) emit('close') }">
    <v-card :title="t(currentRequest?.action === 'remove' ? 'customHome.confirmRemove' : 'customHome.chooseContribution')">
      <v-card-text>
        <v-alert v-if="errorMessage" type="error" role="alert">{{ errorMessage }}</v-alert>
        <v-select v-if="currentRequest?.kind === 'banners'" v-model="bannerSources" multiple
          :items="snapshot?.bannerSources ?? []" :disabled="busy" :label="t('customHome.removeBannerSources')" />
        <p v-else-if="currentRequest?.kind === 'category'">{{ currentRequest.category.label }}</p>
        <v-select v-else-if="blocks.length > 1" v-model="blockId" :items="choices" :disabled="busy"
          :label="t('customHome.chooseContribution')" />
        <p v-else-if="selectedBlock">{{ selectedBlock.reference.sourceId }} — {{ selectedBlock.reference.labelSnapshot }}</p>
        <p v-if="currentRequest?.action === 'remove' && selectedBlock?.kind === 'category-content'">
          {{ t('customHome.removeCategoryBlockWarning') }}
        </p>
        <p v-else-if="currentRequest?.action === 'remove'">{{ t('customHome.removeContentWarning') }}</p>
      </v-card-text>
      <v-card-actions>
        <v-btn :disabled="busy" @click="emit('close')">{{ t('customHome.cancel') }}</v-btn>
        <v-btn :disabled="!canApply" :loading="busy" @click="apply">
          {{ t(currentRequest?.action === 'remove' ? 'customHome.removeFromHome' : currentRequest?.action === 'up' ? 'customHome.moveUp' : 'customHome.moveDown') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>