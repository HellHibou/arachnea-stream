<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import App from '@/App.vue'
import { useI18n } from '@/i18n'
import { provideProfiles, useProfiles } from '@/composables/useProfiles'
import ProfileOnboarding from './ProfileOnboarding.vue'
import ProfileSwitcher from './ProfileSwitcher.vue'

const profiles = useProfiles()
provideProfiles(profiles)
const { t } = useI18n()
const creating = ref(false)
const additionalCreation = ref(false)
const needsOnboarding = computed(() => {
  const list = profiles.list.value
  return Boolean(list && list.used_slots === 0 && !profiles.pendingOnly.value)
})
watch(() => profiles.activeProfile.value, (profile) => {
  if (profile) {
    creating.value = false
    additionalCreation.value = false
  }
})
watch(needsOnboarding, (value) => { if (value) creating.value = true }, { immediate: true })

/** Shows the reusable guide for an additional profile while the selector stays mounted. */
function startCreation(): void {
  profiles.clearError()
  additionalCreation.value = true
  creating.value = true
}
</script>

<template>
  <div v-if="!profiles.initialized.value || profiles.loading.value" class="profile-gate" role="status">
    {{ t('profiles.loading') }}
  </div>
  <div v-else-if="!profiles.list.value" class="profile-gate" role="alert">
    <p>{{ t(`profiles.errors.${profiles.errorCode.value ?? 'storage_unavailable'}`) }}</p>
    <v-btn @click="profiles.retry()">{{ t('profiles.retry') }}</v-btn>
  </div>
  <App v-else-if="profiles.activeProfile.value" :key="profiles.activeId.value ?? undefined" />
  <div v-else-if="creating || needsOnboarding" class="profile-gate">
    <ProfileOnboarding :additional="additionalCreation" />
  </div>
  <div v-else class="profile-gate">
    <ProfileSwitcher @create="startCreation" />
  </div>
</template>

<style scoped>
.profile-gate {
  display: grid;
  place-items: center;
  min-height: 100dvh;
  padding: 24px;
}
</style>
