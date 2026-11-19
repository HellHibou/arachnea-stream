<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from '@/i18n'
import { injectProfiles } from '@/composables/useProfiles'
import { useAccountSession } from '@/composables/useAccountSession'
import type { ProfileView } from '@/services/profileApi'
import ProfileAvatar from './ProfileAvatar.vue'
import ProfilePasswordForm from './ProfilePasswordForm.vue'
import ProfileEditor from './ProfileEditor.vue'

const props = withDefaults(defineProps<{ manage?: boolean; selectOnly?: boolean }>(), { manage: false, selectOnly: false })
const emit = defineEmits<{ create: []; selected: [profileId: string] }>()
const profiles = injectProfiles()
const accounts = useAccountSession()
const { t } = useI18n()

const target = ref<ProfileView | null>(null)
const confirmOpen = ref(false)
const editing = ref<ProfileView | null>(null)

/** Opens a detached identity editor. @param profile Profile to edit. */
function requestEdit(profile: ProfileView): void {
  profiles.clearError()
  editing.value = profile
}

const quotaReached = computed(() => {
  const list = profiles.list.value
  return !list || list.used_slots >= list.max_profiles_per_user
})

/** Opens the confirmation for one profile deletion. @param profile Profile to delete. */
function requestDelete(profile: ProfileView): void {
  target.value = profile
  confirmOpen.value = true
}

/** Deletes the confirmed profile. */
async function confirmDelete(): Promise<void> {
  if (!target.value) return
  const deleted = await profiles.deleteProfile(target.value)
  if (deleted) {
    confirmOpen.value = false
    target.value = null
  }
}

/** Selects one profile and reports completion to a containing dialog. @param profileId Selected profile UUID. */
async function selectProfile(profileId: string): Promise<void> {
  await profiles.select(profileId)
  if (profiles.activeId.value === profileId) emit('selected', profileId)
}

/** Revokes the account session; the gate exposes any failure. */
async function signOut(): Promise<void> {
  try { await accounts.logout() } catch { /* Shared session error remains visible. */ }
}
</script>

<template>
  <section class="profile-switcher" :class="{ 'profile-switcher--select': props.selectOnly }" :aria-label="props.manage ? t('profiles.manageTitle') : t('profiles.selectTitle')">
    <header class="profile-switcher__header">
      <div>
        <h1 v-if="!props.manage">{{ t('profiles.selectTitle') }}</h1>
        <h1 v-else-if="props.selectOnly">{{ t('profiles.selectTitle') }}</h1>
        <p v-if="!props.selectOnly" class="profile-switcher__quota">{{ t('profiles.quota', { used: profiles.list.value?.used_slots ?? 0, max: profiles.list.value?.max_profiles_per_user ?? 0 }) }}</p>
      </div>
      <button v-if="!props.selectOnly" class="profile-switcher__create" type="button" :disabled="quotaReached || profiles.busy.value" @click="emit('create')">
        {{ t('profiles.createTitle') }}
      </button>
    </header>

    <p v-if="profiles.errorCode.value" class="profile-switcher__error" role="alert">{{ t(`profiles.errors.${profiles.errorCode.value}`) }}</p>
    <p v-else-if="quotaReached" class="profile-switcher__notice">{{ t('profiles.quotaReached') }}</p>

    <ul v-if="profiles.hasProfiles.value" class="profile-switcher__list">
      <li
        v-for="profile in profiles.list.value?.profiles.filter((candidate) => candidate.state === 'active') ?? []"
        :key="profile.profile_id"
        class="profile-switcher__item"
        :class="{ 'profile-switcher__item--active': profile.profile_id === profiles.activeId.value }"
      >
        <ProfileAvatar :avatar-id="profile.avatar_id" :catalogue="profiles.avatars.value" :name="profile.name" :size="40" />
        <button v-if="!props.manage" class="profile-switcher__name" type="button" :disabled="profiles.busy.value" @click="selectProfile(profile.profile_id)">
          {{ profile.name }}
        </button>
        <span v-else class="profile-switcher__name">{{ profile.name }}</span>
        <button v-if="props.manage && !props.selectOnly" class="profile-switcher__edit" type="button" :disabled="profiles.busy.value" :aria-label="t('profiles.editProfile', { name: profile.name })" @click="requestEdit(profile)">
          {{ t('profiles.edit') }}
        </button>
        <button v-if="!props.selectOnly" class="profile-switcher__delete" type="button" :disabled="profiles.busy.value" :aria-label="t('profiles.deleteProfile', { name: profile.name })" @click="requestDelete(profile)">
          {{ t('profiles.delete') }}
        </button>
      </li>
    </ul>
    <div v-else-if="profiles.pendingOnly.value" class="profile-switcher__pending" role="status">
      <p>{{ t('profiles.preparing') }}</p>
      <button v-if="!profiles.hasPendingDeletions.value" type="button" :disabled="profiles.loading.value || profiles.busy.value" @click="profiles.retry()">{{ t('profiles.retry') }}</button>
    </div>
    <p v-else class="profile-switcher__notice">{{ t('profiles.noProfiles') }}</p>

    <div v-if="profiles.hasPendingDeletions.value" class="profile-switcher__pending" role="status">
      <p>{{ t('profiles.deletionPending') }}</p>
      <button type="button" :disabled="profiles.busy.value || profiles.loading.value" @click="profiles.retryDeletions()">
        {{ t('profiles.retryDeletions') }}
      </button>
    </div>

    <footer v-if="!props.manage" class="profile-switcher__actions">
      <button class="profile-switcher__signout" type="button" :disabled="accounts.busy.value" @click="signOut">{{ t('accounts.signOut') }}</button>
      <slot name="actions" />
    </footer>
    <ProfilePasswordForm v-if="props.manage" />
  </section>

  <v-dialog :model-value="Boolean(editing)" :persistent="profiles.busy.value" max-width="720" scrollable class="profile-editor-dialog" @update:model-value="!$event && (editing = null)">
    <ProfileEditor v-if="editing" :key="editing.profile_id" :profile="editing" @saved="editing = null" @cancel="editing = null" />
  </v-dialog>

  <v-dialog v-model="confirmOpen" max-width="420">
    <v-card>
      <v-card-title>{{ t('profiles.deleteTitle') }}</v-card-title>
      <v-card-text>
        <p>{{ target ? t('profiles.deleteConfirm', { name: target.name }) : '' }}</p>
        <p v-if="profiles.errorCode.value" class="profile-switcher__error" role="alert">{{ t(`profiles.errors.${profiles.errorCode.value}`) }}</p>
      </v-card-text>
      <v-card-actions>
        <v-btn variant="text" :disabled="profiles.busy.value" @click="confirmOpen = false">{{ t('profiles.cancel') }}</v-btn>
        <v-btn color="error" :loading="profiles.busy.value" @click="confirmDelete">{{ t('profiles.delete') }}</v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.profile-editor-dialog :deep(.v-overlay__scrim) {
  backdrop-filter: var(--backdrop-filter-strong);
  opacity: 1;
  background: rgb(0 0 0 / 45%);
}

.profile-switcher {
  display: grid;
  gap: 20px;
  width: min(560px, 100%);
  margin: auto;
  padding: 32px 24px;
}

.profile-switcher__header {
  display: flex;
  align-items: start;
  justify-content: space-between;
  gap: 16px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border-color-primary);
}

.profile-switcher__header h1 {
  position: relative;
  margin: 0 0 6px;
  padding-inline-start: 12px;
  font-size: 1.16rem;
  font-weight: 800;
  letter-spacing: 0.01em;
  line-height: 1.2;
}

.profile-switcher__header h1::before {
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

.profile-switcher--select .profile-switcher__header {
  justify-content: flex-start;
}

.profile-switcher--select .profile-switcher__list {
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 160px), 1fr));
}

.profile-switcher__actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.profile-switcher__quota,
.profile-switcher__notice,
.profile-switcher__pending p {
  margin: 0;
}

.profile-switcher__error {
  margin: 0;
  color: var(--text-danger);
}

.profile-switcher__list {
  display: grid;
  gap: 12px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.profile-switcher__item {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 12px 16px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  box-shadow: var(--inset-light);
}

.profile-switcher__item--active {
  border-color: var(--color-primary);
}

.profile-switcher__name {
  flex: 1;
  min-width: 0;
  padding: 6px 0;
  border: 0;
  background: transparent;
  color: var(--text-primary);
  font: inherit;
  font-weight: 600;
  text-align: left;
  overflow-wrap: anywhere;
  cursor: pointer;
}

.profile-switcher__create,
.profile-switcher__edit,
.profile-switcher__delete,
.profile-switcher__signout,
.profile-switcher__pending button {
  min-height: 36px;
  padding: 8px 14px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
  cursor: pointer;
}

.profile-switcher__create,
.profile-switcher__signout {
  border-color: var(--color-primary);
}

.profile-switcher__signout {
  justify-self: center;
  padding: 0 20px;
  border: 0;
  border-radius: var(--radius);
  background: var(--bg-accent-primary);
  color: var(--text-primary);
  font-size: 0.98rem;
  font-weight: 700;
  letter-spacing: 0.01em;
}

.profile-switcher__signout:hover {
  filter: brightness(1.04);
}

.profile-switcher__delete {
  color: var(--text-danger);
  border-color: var(--border-color-primary);
}

.profile-switcher__create:focus-visible,
.profile-switcher__edit:focus-visible,
.profile-switcher__delete:focus-visible,
.profile-switcher__signout:focus-visible,
.profile-switcher__name:focus-visible,
.profile-switcher__pending button:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.profile-switcher__create:disabled,
.profile-switcher__edit:disabled,
.profile-switcher__delete:disabled,
.profile-switcher__name:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.profile-switcher__pending {
  display: flex;
  align-items: center;
  gap: 12px;
}

@media (max-width: 720px) {
  .profile-switcher__header {
    flex-wrap: wrap;
  }

  .profile-switcher__item {
    flex-wrap: wrap;
  }

  .profile-switcher__delete {
    margin-inline-start: auto;
  }
}
</style>
