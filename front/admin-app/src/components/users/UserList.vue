<script setup lang="ts">
import type { PublicUser } from '@/services/adminApi'
import { useI18n } from '@/i18n'

/** Safe rows and operation state; busy defaults to false. */
withDefaults(defineProps<{ users: readonly PublicUser[]; busy?: boolean }>(), { busy: false })
const emit = defineEmits<{ reset: [user: PublicUser]; remove: [user: PublicUser] }>()
const { t } = useI18n()
</script>

<template>
  <v-list>
    <v-list-item v-for="user in users" :key="user.user_id" :title="user.login" :subtitle="t(user.state === 'deleting' ? 'users.deleting' : user.must_change_password ? 'users.changeRequired' : 'users.active')">
      <template #append>
        <v-btn variant="text" :disabled="busy || user.state !== 'active'" @click="emit('reset', user)">{{ t('users.reset') }}</v-btn>
        <v-btn variant="text" color="error" :disabled="busy" @click="emit('remove', user)">{{ t('users.delete') }}</v-btn>
      </template>
    </v-list-item>
  </v-list>
  <p v-if="!users.length">{{ t('users.empty') }}</p>
</template>
