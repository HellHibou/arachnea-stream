<script setup lang="ts">
import { computed, nextTick, ref } from 'vue'
import { useAccountSession } from '@/composables/useAccountSession'
import { injectProfiles } from '@/composables/useProfiles'
import { useI18n } from '@/i18n'
import ProfileAvatar from './ProfileAvatar.vue'
import ProfileSwitcherDialog from './ProfileSwitcherDialog.vue'

const profiles = injectProfiles()
const accounts = useAccountSession()
const { t } = useI18n()

const menuOpen = ref(false)
const selectOpen = ref(false)
const manageOpen = ref(false)

/** Selects one profile and lets the gate remount the application shell. @param profileId Selected profile UUID. */
async function selectProfile(profileId: string): Promise<void> {
  menuOpen.value = false
  await profiles.select(profileId)
}

/** Opens the profile selection dialog after closing the menu. */
function openSelection(): void {
  menuOpen.value = false
  void nextTick(() => { selectOpen.value = true })
}

/** Opens the profile management dialog after closing the menu. */
function openManagement(): void {
  menuOpen.value = false
  void nextTick(() => { manageOpen.value = true })
}

/** Revokes the account session. */
async function signOut(): Promise<void> {
  menuOpen.value = false
  try { await accounts.logout() } catch { /* Session error is rendered by the gate. */ }
}
</script>

<template>
  <v-menu v-model="menuOpen" :close-on-content-click="true">
    <template #activator="{ props: activatorProps }">
      <button
        v-bind="activatorProps"
        class="profile-menu__trigger"
        type="button"
        :aria-label="t('profiles.menuLabel')"
        :aria-expanded="menuOpen"
      >
        <ProfileAvatar
          v-if="profiles.activeProfile.value"
          :avatar-id="profiles.activeProfile.value.avatar_id"
          :catalogue="profiles.avatars.value"
          :name="profiles.activeProfile.value.name"
          :size="32"
        />
        <v-icon v-else icon="mdi-account-circle" size="24" aria-hidden="true" />
      </button>
    </template>

    <v-list class="profile-menu__list" density="compact">
      <v-list-item class="profile-menu__current" disabled>
        <template #prepend>
          <ProfileAvatar
            v-if="profiles.activeProfile.value"
            :avatar-id="profiles.activeProfile.value.avatar_id"
            :catalogue="profiles.avatars.value"
            :name="profiles.activeProfile.value.name"
            :size="32"
          />
        </template>
        <v-list-item-title>{{ profiles.activeProfile.value?.name }}</v-list-item-title>
      </v-list-item>

      <v-divider />

      <v-list-item class="profile-menu__item" @click="openSelection">
        <v-list-item-title>{{ t('profiles.switch') }}</v-list-item-title>
      </v-list-item>

      <v-list-item class="profile-menu__item" @click="openManagement">
        <v-list-item-title>{{ t('profiles.manage') }}</v-list-item-title>
      </v-list-item>

      <v-divider />

      <v-list-item class="profile-menu__item" @click="signOut">
        <v-list-item-title>{{ t('accounts.signOut') }}</v-list-item-title>
      </v-list-item>
    </v-list>
  </v-menu>

  <ProfileSwitcherDialog v-model:open="selectOpen" mode="select" />
  <ProfileSwitcherDialog v-model:open="manageOpen" mode="manage" />
</template>

<style scoped>
.profile-menu__trigger {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 52px;
  min-height: var(--control-height);
  padding: 0;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  color: var(--text-primary);
  cursor: pointer;
  box-shadow:
    var(--shadow-heavy),
    var(--inset-light);
}

.profile-menu__trigger:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.profile-menu__list {
  min-width: 220px;
  background: var(--bg-surface);
  backdrop-filter: var(--backdrop-filter-medium);
  -webkit-backdrop-filter: var(--backdrop-filter-medium);
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  box-shadow: var(--shadow-heavy);
  overflow: hidden;
}

.profile-menu__item,
.profile-menu__current {
  cursor: pointer;
}

:deep(.v-overlay__content) {
  background: transparent;
  box-shadow: none;
}
</style>