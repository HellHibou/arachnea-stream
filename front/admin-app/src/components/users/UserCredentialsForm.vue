<script setup lang="ts">
import { createOperationId } from '@/services/operationId'
import { ref, watch } from 'vue'
import { useI18n } from '@/i18n'

/** Reset mode uses the supplied login. Busy defaults to false. */
const props = withDefaults(defineProps<{ resetLogin?: string; busy?: boolean }>(), { resetLogin: '', busy: false })
const emit = defineEmits<{ submit: [operationId: string, login: string, password: string] }>()
const { t } = useI18n()
const login = ref(props.resetLogin)
const password = ref('')
const operationId = ref(createOperationId())
watch([login, password], () => { operationId.value = createOperationId() })
/** Emits the credential draft with a retry UUID, without persisting the secret. */
function submit(): void { emit('submit', operationId.value, login.value, password.value) }
</script>

<template>
  <form @submit.prevent="submit">
    <v-text-field v-model="login" :label="t('users.login')" autocomplete="off" minlength="3" maxlength="64" required :disabled="busy || Boolean(resetLogin)" />
    <v-text-field v-model="password" :label="t('users.temporaryPassword')" type="password" autocomplete="new-password" minlength="12" maxlength="512" required :disabled="busy" />
    <p class="mb-4">{{ t('users.passwordHint') }}</p>
    <v-btn type="submit" color="primary" :loading="busy" :disabled="busy">{{ t(resetLogin ? 'users.reset' : 'users.create') }}</v-btn>
  </form>
</template>
