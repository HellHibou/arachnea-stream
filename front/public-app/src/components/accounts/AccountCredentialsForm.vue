<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { t } from '@/i18n'
import { PASSWORD_MAX_LENGTH, PASSWORD_MIN_LENGTH } from '@/services/password'
import PasswordStrengthMeter from './PasswordStrengthMeter.vue'

/** Form mode and operation state; no network calls or shared preference writes. */
const props = withDefaults(defineProps<{ changePassword?: boolean; busy?: boolean; errorCode?: string | null }>(), { changePassword: false, busy: false, errorCode: null })
const emit = defineEmits<{ submit: [login: string, password: string, replacement: string] }>()
const login = ref('')
const password = ref('')
const replacement = ref('')
const confirmation = ref('')
const mismatch = ref(false)
const samePassword = ref(false)

/** Hides the revoked-session notice because the login gate already conveys that state. */
const formErrorCode = computed(() => props.errorCode === 'session_revoked' ? null : props.errorCode)

/** Clears transient validation once either credential draft changes. */
watch([password, replacement, confirmation], () => {
  mismatch.value = false
  samePassword.value = false
})

/** Validates confirmation and asks for confirmation before reusing the current password. */
function submit(): void {
  mismatch.value = props.changePassword && replacement.value !== confirmation.value
  if (mismatch.value) return
  samePassword.value = props.changePassword && replacement.value === password.value
  if (samePassword.value) return
  emit('submit', login.value, password.value, replacement.value)
}

/** Submits the replacement after the explicit same-password confirmation. */
function confirmSamePassword(): void {
  samePassword.value = false
  emit('submit', login.value, password.value, replacement.value)
}
</script>

<template>
  <form class="account-form" @submit.prevent="submit">
    <h1>{{ t(changePassword ? 'accounts.changeTitle' : 'accounts.loginTitle') }}</h1>
    <p v-if="changePassword">{{ t('accounts.changeRequired') }}</p>
    <v-text-field v-if="!changePassword" v-model="login" :label="t('accounts.login')" autocomplete="username" maxlength="64" required :disabled="busy" />
    <v-text-field v-model="password" :label="t(changePassword ? 'accounts.currentPassword' : 'accounts.password')" type="password" autocomplete="current-password" :maxlength="PASSWORD_MAX_LENGTH" required :disabled="busy" />
    <template v-if="changePassword">
      <v-text-field v-model="replacement" :label="t('accounts.newPassword')" type="password" autocomplete="new-password" :minlength="PASSWORD_MIN_LENGTH" :maxlength="PASSWORD_MAX_LENGTH" required :disabled="busy" />
      <PasswordStrengthMeter :password="replacement" />
      <v-text-field v-model="confirmation" :label="t('accounts.confirmPassword')" type="password" autocomplete="new-password" :maxlength="PASSWORD_MAX_LENGTH" required :disabled="busy" />
    </template>
    <p v-if="mismatch" role="alert">{{ t('accounts.passwordMismatch') }}</p>
    <v-alert v-if="samePassword" type="warning" variant="tonal" class="mb-4">
      <p>{{ t('accounts.samePasswordWarning') }}</p>
      <div class="same-password-actions">
        <v-btn size="small" color="warning" variant="flat" @click="confirmSamePassword">{{ t('accounts.samePasswordConfirm') }}</v-btn>
        <v-btn size="small" variant="text" @click="samePassword = false">{{ t('accounts.cancel') }}</v-btn>
      </div>
    </v-alert>
    <p v-if="formErrorCode" role="alert">{{ t(`accounts.errors.${formErrorCode}`) }}</p>
    <p v-if="changePassword">{{ t('accounts.passwordHint') }}</p>
    <v-btn type="submit" color="primary" :loading="busy" :disabled="busy || samePassword">{{ t(changePassword ? 'accounts.savePassword' : 'accounts.signIn') }}</v-btn>
  </form>
</template>

<style scoped>
.account-form {
  width: min(420px, 100%);
  margin: auto;
  padding: 24px;
}

.account-form h1 {
  margin-bottom: 20px;
}

.account-form p {
  margin-block: 12px;
}

.same-password-actions {
  display: flex;
  gap: 8px;
  margin-top: 8px;
}
</style>
