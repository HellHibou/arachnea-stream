<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from '@/i18n'
import { useUserManagement } from '@/composables/useUserManagement'
import type { PublicUser } from '@/services/adminApi'
import UserCredentialsDialog from './UserCredentialsDialog.vue'
import UserList from './UserList.vue'

const { t } = useI18n()
const { users, busy, errorCode, load, create, reset, remove, setPasswordChange, clearError } =
  useUserManagement()
const createOpen = ref(false)
const resetTarget = ref<PublicUser | null>(null)
const deleteTarget = ref<PublicUser | null>(null)

onMounted(() => void load())

/**
 * Creates an account and closes the dialog only once the server confirmed it.
 *
 * @param login - Canonical login chosen by the administrator.
 * @param password - Temporary password kept in memory only.
 * @param mustChange - Whether the next sign-in must replace the password.
 */
async function submitCreate(
  login: string,
  password: string,
  mustChange: boolean,
): Promise<void> {
  if (await create(login, password, mustChange)) createOpen.value = false
}

/**
 * Resets the selected account without retaining the submitted password.
 *
 * @param _login - Display-only login echoed by the dialog.
 * @param password - Temporary password kept in memory only.
 */
async function submitReset(_login: string, password: string): Promise<void> {
  if (resetTarget.value && (await reset(resetTarget.value.user_id, password))) {
    resetTarget.value = null
  }
}

/**
 * Persists the mandatory-replacement option toggled for one row.
 *
 * @param user - Account row whose flag changed.
 * @param mustChange - New value of the flag.
 */
async function togglePasswordChange(user: PublicUser, mustChange: boolean): Promise<void> {
  await setPasswordChange(user.user_id, mustChange)
}

/** Opens account creation from a clean form state. */
function openCreate(): void {
  clearError()
  createOpen.value = true
}

/**
 * Opens the password reset dialog for one account from a clean form state.
 *
 * @param user - Account selected by the administrator.
 */
function openReset(user: PublicUser): void {
  clearError()
  resetTarget.value = user
}

/**
 * Opens the destructive-action confirmation for one account.
 *
 * @param user - Account selected by the administrator.
 */
function confirmDelete(user: PublicUser): void {
  deleteTarget.value = user
}

/** Deletes the confirmed account by UUID. */
async function submitDelete(): Promise<void> {
  if (deleteTarget.value && (await remove(deleteTarget.value.user_id))) {
    deleteTarget.value = null
  }
}
</script>

<template>
  <v-container fluid>
    <div class="d-flex align-center justify-space-between ga-4 mb-6">
      <h1 class="text-h4">{{ t('users.title') }}</h1>
      <v-btn prepend-icon="mdi-refresh" :loading="busy" @click="load">{{ t('users.refresh') }}</v-btn>
    </div>
    <v-alert v-if="errorCode" type="error" class="mb-4" role="alert">
      {{ t(`users.errors.${errorCode}`) }}
    </v-alert>
    <v-card :title="t('users.list')">
      <v-card-text>
        <div class="users-toolbar">
          <v-btn
            variant="text"
            prepend-icon="mdi-plus"
            :disabled="busy"
            @click="openCreate"
          >
            {{ t('users.addUser') }}
          </v-btn>
        </div>
        <UserList
          :users="users"
          :busy="busy"
          @reset="openReset"
          @remove="confirmDelete"
          @password-change="togglePasswordChange"
        />
      </v-card-text>
    </v-card>

    <UserCredentialsDialog
      :model-value="createOpen"
      :busy="busy"
      :error-code="errorCode"
      @submit="submitCreate"
      @close="createOpen = false"
    />
    <UserCredentialsDialog
      :model-value="Boolean(resetTarget)"
      :reset-login="resetTarget?.login ?? ''"
      :busy="busy"
      :error-code="errorCode"
      @submit="submitReset"
      @close="resetTarget = null"
    />

    <v-dialog :model-value="Boolean(deleteTarget)" max-width="480" persistent>
      <v-card :title="t('users.delete')">
        <v-card-text>
          {{ t('users.deleteConfirmation', { login: deleteTarget?.login ?? '' }) }}
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn :disabled="busy" @click="deleteTarget = null">{{ t('users.cancel') }}</v-btn>
          <v-btn color="error" :loading="busy" :disabled="busy" @click="submitDelete">
            {{ t('users.delete') }}
          </v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </v-container>
</template>

<style scoped>
.users-toolbar {
  display: flex;
  gap: 4px;
  margin-bottom: 8px;
}
</style>
