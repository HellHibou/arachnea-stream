<script setup lang="ts">
import { onMounted, ref } from 'vue'
import Parameters from '@/components/Parameters.vue'
import { usePreferenceDraft } from '@/composables/usePreferenceDraft'
import { useI18n } from '@/i18n'

const emit = defineEmits<{ close: [] }>()
const { t } = useI18n()
const draft = usePreferenceDraft()
const confirmingDiscard = ref(false)
onMounted(() => { void draft.load() })

/** Confirms before discarding a dirty draft. */
function requestClose(): void {
  if (draft.saving.value) return
  if (draft.dirty.value) confirmingDiscard.value = true
  else emit('close')
}

/** Closes only after the entire save and authoritative refresh have succeeded. */
async function save(): Promise<void> {
  if (await draft.save()) emit('close')
}
</script>

<template>
  <v-dialog :model-value="true" max-width="720" scrollable class="parameters-dialog"
    :persistent="draft.dirty.value || draft.saving.value || draft.blocked.value"
    @update:model-value="!$event && requestClose()" @keydown.esc="requestClose">
    <v-card class="parameters-dialog__surface" :title="t('navigation.settings')" :aria-busy="draft.loading.value || draft.saving.value">
      <v-card-text>
        <p v-if="draft.loading.value" role="status">{{ t('settings.draft.loading') }}</p>
        <p v-if="draft.errorCode.value" role="alert">{{ t(`profiles.errors.${draft.errorCode.value}`) }}</p>
        <fieldset v-if="draft.ready.value" class="parameters-dialog__fields"
          :disabled="draft.loading.value || draft.saving.value || draft.blocked.value || confirmingDiscard">
          <Parameters :parameters="draft.parameters" :home-preferences="draft.homePreferences" />
        </fieldset>
        <div v-if="confirmingDiscard" role="alertdialog" :aria-label="t('settings.draft.discard')" aria-describedby="discard-description">
          <p id="discard-description">{{ t('settings.draft.discard') }}</p>
          <v-btn @click="confirmingDiscard = false">{{ t('settings.draft.continue') }}</v-btn>
          <v-btn @click="emit('close')">{{ t('settings.draft.discardAction') }}</v-btn>
        </div>
      </v-card-text>
      <v-card-actions v-if="!confirmingDiscard" class="parameters-dialog__actions">
        <v-btn :disabled="draft.saving.value" @click="requestClose">{{ t('profiles.cancel') }}</v-btn>
        <template v-if="draft.blocked.value || draft.errorCode.value === 'revision_conflict'">
          <p>{{ t('settings.draft.conflict') }}</p>
          <v-btn :disabled="draft.loading.value" @click="draft.load()">{{ t('settings.draft.reload') }}</v-btn>
          <v-btn :disabled="draft.loading.value" @click="draft.load(true)">{{ t('settings.draft.reapply') }}</v-btn>
        </template>
        <v-btn v-else-if="!draft.ready.value"
          :disabled="draft.loading.value" @click="draft.load()">{{ t('profiles.retry') }}</v-btn>
        <v-btn v-else :disabled="draft.loading.value || draft.saving.value" @click="save">
          {{ t(draft.saving.value ? 'profiles.saving' : 'customHome.save') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.parameters-dialog__surface {
  background: var(--bg-surface);
  color: var(--text-primary);
  font-family: inherit;
}

.parameters-dialog__fields {
  min-width: 0;
  padding: 0;
  border: 0;
}

.parameters-dialog__actions {
  flex-wrap: wrap;
}

.parameters-dialog :deep(.v-overlay__scrim) {
  backdrop-filter: var(--backdrop-filter-strong);
  opacity: 1;
  background: rgb(0 0 0 / 45%);
}
</style>
