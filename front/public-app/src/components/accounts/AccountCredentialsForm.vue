<script setup lang="ts">
import { ref } from 'vue'
import { t } from '@/i18n'

/** Form mode and operation state; no network calls or shared preference writes. */
const props = withDefaults(defineProps<{ changePassword?: boolean; busy?: boolean; errorCode?: string | null }>(), { changePassword: false, busy: false, errorCode: null })
const emit = defineEmits<{ submit: [login: string, password: string, replacement: string] }>()
const login = ref('')
const password = ref('')
const replacement = ref('')
const confirmation = ref('')
const mismatch = ref(false)
/** Validates password confirmation and emits the in-memory credentials. */
function submit(): void {
  mismatch.value = props.changePassword && replacement.value !== confirmation.value
  if (!mismatch.value) emit('submit', login.value, password.value, replacement.value)
}
</script>

<template>
  <form class="account-form" @submit.prevent="submit">
    <h1>{{ t(changePassword ? 'accounts.changeTitle' : 'accounts.loginTitle') }}</h1>
    <p v-if="changePassword">{{ t('accounts.changeRequired') }}</p>
    <v-text-field v-if="!changePassword" v-model="login" :label="t('accounts.login')" autocomplete="username" maxlength="64" required :disabled="busy" />
    <v-text-field v-model="password" :label="t(changePassword ? 'accounts.currentPassword' : 'accounts.password')" type="password" autocomplete="current-password" maxlength="512" required :disabled="busy" />
    <template v-if="changePassword">
      <v-text-field v-model="replacement" :label="t('accounts.newPassword')" type="password" autocomplete="new-password" minlength="12" maxlength="512" required :disabled="busy" />
      <v-text-field v-model="confirmation" :label="t('accounts.confirmPassword')" type="password" autocomplete="new-password" required :disabled="busy" />
    </template>
    <p v-if="mismatch" role="alert">{{ t('accounts.passwordMismatch') }}</p>
    <p v-if="errorCode" role="alert">{{ t(`accounts.errors.${errorCode}`) }}</p>
    <p>{{ t('accounts.passwordHint') }}</p>
    <v-btn type="submit" color="primary" :loading="busy" :disabled="busy">{{ t(changePassword ? 'accounts.savePassword' : 'accounts.signIn') }}</v-btn>
  </form>
</template>

<style scoped>
.account-form { width: min(420px, 100%); margin: auto; padding: 24px; }
.account-form h1 { margin-bottom: 20px; }
.account-form p { margin-block: 12px; }
</style>
