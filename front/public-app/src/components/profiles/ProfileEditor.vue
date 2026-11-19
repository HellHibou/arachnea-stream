<script setup lang="ts">
import { computed, ref, useId } from 'vue'
import { useI18n } from '@/i18n'
import { injectProfiles } from '@/composables/useProfiles'
import { ProfileApiError, profileSessionCall, type ProfileSnapshot, type ProfileView } from '@/services/profileApi'
import ProfileForm from './ProfileForm.vue'

/** @property profile - Required identity captured when the editor opens. */
const props = defineProps<{ profile: ProfileView }>()
const emit = defineEmits<{ saved: []; cancel: [] }>()
const profiles = injectProfiles()
const { t } = useI18n()
const original = ref(props.profile)
const name = ref(props.profile.name)
const avatarId = ref(props.profile.avatar_id)
const reloading = ref(false)
const reloadError = ref<string | null>(null)
const formId = useId()
const saveDisabled = computed(() => profiles.busy.value || reloading.value || !name.value.trim() || name.value.trim().length > 64 || !avatarId.value)

/** Saves the detached identity draft and closes only after confirmation. */
async function save(): Promise<void> {
  reloadError.value = null
  if (await profiles.updateProfile(original.value, name.value, avatarId.value)) emit('saved')
}

/** Discards the conflicted draft and loads the latest server identity explicitly. */
async function reload(): Promise<void> {
  reloading.value = true
  reloadError.value = null
  try {
    const snapshot = await profileSessionCall<ProfileSnapshot>('get', { profile_id: original.value.profile_id })
    original.value = snapshot.profile
    name.value = snapshot.profile.name
    avatarId.value = snapshot.profile.avatar_id
    profiles.clearError()
  } catch (error) {
    reloadError.value = error instanceof ProfileApiError ? error.code : 'storage_unavailable'
  } finally {
    reloading.value = false
  }
}
</script>

<template>
  <v-card class="profile-editor__surface">
    <v-card-title class="profile-editor__header">
      <h2 class="profile-editor__title">{{ t('profiles.editTitle') }}</h2>
    </v-card-title>
    <v-card-text>
      <ProfileForm
        :id="formId"
        class="profile-editor"
        v-model:name="name"
        v-model:avatar-id="avatarId"
        :catalogue="profiles.avatars.value"
        :busy="profiles.busy.value || reloading"
        :error-code="reloadError ?? profiles.errorCode.value"
        :show-submit="false"
        @submit="save"
      >
        <v-btn v-if="profiles.errorCode.value === 'revision_conflict'" variant="text" :disabled="profiles.busy.value || reloading" @click="reload">
          {{ t('profiles.reload') }}
        </v-btn>
      </ProfileForm>
    </v-card-text>
    <v-card-actions class="profile-editor__actions">
      <v-btn variant="flat" :disabled="profiles.busy.value || reloading" @click="emit('cancel')">{{ t('profiles.cancel') }}</v-btn>
      <v-btn variant="flat" type="submit" :form="formId" :disabled="saveDisabled">{{ t(profiles.busy.value ? 'profiles.saving' : 'profiles.save') }}</v-btn>
    </v-card-actions>
  </v-card>
</template>

<style scoped>
.profile-editor__surface {
  background: var(--bg-surface);
  color: var(--text-primary);
  font-family: inherit;
}

.profile-editor {
  width: 100%;
}

.profile-editor__header {
  margin: 16px 24px 0;
  padding: 0 2px 10px;
  border-bottom: 1px solid var(--border-color-primary);
}

.profile-editor__title {
  position: relative;
  margin: 0;
  padding-inline-start: 12px;
  color: var(--text-primary);
  font-size: 1.16rem;
  font-weight: 800;
  letter-spacing: 0.01em;
  line-height: 1.2;
  white-space: normal;
}

.profile-editor__title::before {
  position: absolute;
  top: 50%;
  inset-inline-start: 0;
  width: 4px;
  height: 18px;
  border-radius: 999px;
  background: var(--bg-accent-marker-primary);
  content: '';
  transform: translateY(-50%);
}

.profile-editor__actions {
  justify-content: center;
  flex-wrap: nowrap;
}

.profile-editor__actions :deep(.v-btn) {
  min-height: 40px;
  padding: 0 20px;
  border-radius: 999px;
  background: var(--bg-accent-primary);
  color: var(--text-primary);
  font: inherit;
  font-weight: 700;
}

.profile-editor__actions :deep(.v-btn:focus-visible) {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}
</style>
