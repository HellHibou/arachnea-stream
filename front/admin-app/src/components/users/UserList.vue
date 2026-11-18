<script setup lang="ts">
import type { PublicUser } from '@/services/adminApi'
import { useI18n } from '@/i18n'

/** Safe rows and operation state; busy defaults to false. */
withDefaults(defineProps<{ users: readonly PublicUser[]; busy?: boolean }>(), { busy: false })
const emit = defineEmits<{
  reset: [user: PublicUser]
  remove: [user: PublicUser]
  passwordChange: [user: PublicUser, mustChange: boolean]
}>()
const { t } = useI18n()
</script>

<template>
  <v-table class="users-table" hover>
    <thead>
      <tr>
        <th>{{ t('users.login') }}</th>
        <th>{{ t('users.mustChangeColumn') }}</th>
        <th class="text-right">{{ t('users.resetColumn') }}</th>
        <th class="text-right">{{ t('users.deleteColumn') }}</th>
      </tr>
    </thead>
    <tbody>
      <tr v-for="user in users" :key="user.user_id">
        <td>
          {{ user.login }}
          <span v-if="user.state === 'deleting'" class="users-table__state">{{ t('users.deleting') }}</span>
        </td>
        <td>
          <v-checkbox-btn
            :model-value="user.must_change_password"
            :disabled="busy || user.state !== 'active'"
            :aria-label="t('users.mustChangeFor', { login: user.login })"
            @update:model-value="(value: boolean | null) => emit('passwordChange', user, value === true)"
          />
        </td>
        <td class="text-right">
          <v-btn
            icon="mdi-lock-reset"
            variant="text"
            :title="t('users.reset')"
            :aria-label="t('users.resetFor', { login: user.login })"
            :disabled="busy || user.state !== 'active'"
            @click="emit('reset', user)"
          />
        </td>
        <td class="text-right">
          <v-btn
            icon="mdi-delete"
            variant="text"
            color="error"
            :title="t('users.delete')"
            :aria-label="t('users.deleteFor', { login: user.login })"
            :disabled="busy"
            @click="emit('remove', user)"
          />
        </td>
      </tr>
    </tbody>
  </v-table>
</template>

<style scoped>
.users-table__state {
  margin-left: 8px;
  font-size: 0.75rem;
  opacity: 0.7;
}
</style>
