<script setup lang="ts">
import { createOperationId } from '@/services/operationId'
import { onMounted, ref } from 'vue'
import { useI18n } from '@/i18n'
import { useUserManagement } from '@/composables/useUserManagement'
import type { PublicUser } from '@/services/adminApi'
import UserCredentialsForm from './UserCredentialsForm.vue'
import UserList from './UserList.vue'

const { t } = useI18n()
const management = useUserManagement()
const resetTarget = ref<PublicUser | null>(null)
const deleteTarget = ref<PublicUser | null>(null)
const deleteId = ref('')
const formKey = ref(0)
onMounted(() => { void management.load() })
/** Creates an account and clears the successful credential draft. @param id Retry identity. @param login Login. @param password Temporary password. */
async function create(id: string, login: string, password: string): Promise<void> { if (await management.create(id, login, password)) formKey.value += 1 }
/** Resets the selected account without retaining a successful password. @param id Retry identity. @param _login Display-only login. @param password Temporary password. */
async function reset(id: string, _login: string, password: string): Promise<void> {
  if (resetTarget.value && await management.reset(id, resetTarget.value.user_id, password)) resetTarget.value = null
}
/** Opens an explicit destructive-action confirmation. @param user Account selected by the administrator. */
function confirmDelete(user: PublicUser): void { deleteTarget.value = user; deleteId.value = createOperationId() }
/** Deletes the confirmed account, retaining its UUID for safe retries. */
async function remove(): Promise<void> {
  if (deleteTarget.value && await management.remove(deleteId.value, deleteTarget.value.user_id)) deleteTarget.value = null
}
</script>

<template>
  <v-container>
    <h1 class="mb-4">{{ t('users.title') }}</h1>
    <v-alert v-if="management.errorCode.value" type="error" class="mb-4" role="alert">{{ t(`users.errors.${management.errorCode.value}`) }}</v-alert>
    <v-card class="mb-6" :title="t('users.create')"><v-card-text><UserCredentialsForm :key="formKey" :busy="management.busy.value" @submit="create" /></v-card-text></v-card>
    <v-card :title="t('users.list')"><v-card-text>
      <v-btn variant="text" :disabled="management.busy.value" @click="management.load">{{ t('users.refresh') }}</v-btn>
      <UserList :users="management.users.value" :busy="management.busy.value" @reset="resetTarget = $event" @remove="confirmDelete" />
    </v-card-text></v-card>
    <v-dialog :model-value="Boolean(resetTarget)" max-width="480" persistent>
      <v-card :title="t('users.reset')"><v-card-text>
        <UserCredentialsForm v-if="resetTarget" :key="resetTarget.user_id" :reset-login="resetTarget.login" :busy="management.busy.value" @submit="reset" />
      </v-card-text><v-card-actions><v-btn :disabled="management.busy.value" @click="resetTarget = null">{{ t('users.cancel') }}</v-btn></v-card-actions></v-card>
    </v-dialog>
    <v-dialog :model-value="Boolean(deleteTarget)" max-width="480" persistent>
      <v-card :title="t('users.delete')"><v-card-text>{{ t('users.deleteConfirmation', { login: deleteTarget?.login ?? '' }) }}</v-card-text>
        <v-card-actions><v-btn :disabled="management.busy.value" @click="deleteTarget = null">{{ t('users.cancel') }}</v-btn><v-btn color="error" :loading="management.busy.value" :disabled="management.busy.value" @click="remove">{{ t('users.delete') }}</v-btn></v-card-actions>
      </v-card>
    </v-dialog>
  </v-container>
</template>
