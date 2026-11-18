<script setup lang="ts">
import { personalDataIssue, clearPersonalDataFailure } from '@/services/personalDataStatus'
import { pendingBookmarkWrites } from '@/services/storage/bookmarks'
import { useStorage } from '@/services/storage'
import { useI18n } from '@/i18n'

const { t } = useI18n()
const storage = useStorage()
/** Retries immutable pending writes without replacing a newer server revision. */
async function retry(): Promise<void> {
  await storage.flushEntryBookmarks()
  if (!pendingBookmarkWrites.value) clearPersonalDataFailure()
}
</script>

<template>
  <aside v-if="personalDataIssue || pendingBookmarkWrites" class="personal-data-status" :role="personalDataIssue ? 'alert' : 'status'">
    <p>{{ t(personalDataIssue ? 'personalData.failed' : 'personalData.saving') }}</p>
    <p v-if="personalDataIssue">{{ personalDataIssue.message }}</p>
    <v-btn v-if="personalDataIssue && pendingBookmarkWrites" variant="text" @click="retry">{{ t('personalData.retry') }}</v-btn>
    <v-btn v-if="personalDataIssue" variant="text" @click="clearPersonalDataFailure">{{ t('personalData.dismiss') }}</v-btn>
  </aside>
</template>

<style scoped>
.personal-data-status {
  position: fixed;
  bottom: 16px;
  left: 16px;
  z-index: 100;
  max-width: min(440px, calc(100vw - 32px));
  padding: 12px 16px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  color: var(--text-primary);
  background: var(--bg-surface);
}
.personal-data-status p {
  margin: 0 0 6px;
}
</style>
