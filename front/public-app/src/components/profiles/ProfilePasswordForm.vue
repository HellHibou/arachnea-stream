<script setup lang="ts">
import { ref, watch } from 'vue'
import { useAccountSession } from '@/composables/useAccountSession'
import { useI18n } from '@/i18n'
import { AccountApiError } from '@/services/accountApi'
import { PASSWORD_MAX_LENGTH, PASSWORD_MIN_LENGTH } from '@/services/password'
import PasswordStrengthMeter from '@/components/accounts/PasswordStrengthMeter.vue'

const accounts = useAccountSession()
const { t } = useI18n()

const currentPassword = ref('')
const newPassword = ref('')
const confirmation = ref('')
const mismatch = ref(false)
const samePassword = ref(false)
const saved = ref(false)
const errorCode = ref<string | null>(null)

watch([currentPassword, newPassword, confirmation], () => {
  mismatch.value = false
  samePassword.value = false
})

/** Submits the replacement once local consistency checks pass. */
async function submit(): Promise<void> {
  mismatch.value = newPassword.value !== confirmation.value
  if (mismatch.value) return
  samePassword.value = newPassword.value === currentPassword.value
  if (samePassword.value) return
  await save()
}

/** Bypasses the same-password warning after explicit confirmation. */
async function confirmSamePassword(): Promise<void> {
  samePassword.value = false
  await save()
}

/** Persists the new password through the account session. */
async function save(): Promise<void> {
  errorCode.value = null
  saved.value = false
  try {
    await accounts.changePassword(currentPassword.value, newPassword.value)
    currentPassword.value = ''
    newPassword.value = ''
    confirmation.value = ''
    saved.value = true
  } catch (error) {
    errorCode.value = error instanceof AccountApiError ? error.code : 'storage_unavailable'
  }
}
</script>

<template>
  <section class="profile-password" :aria-label="t('profiles.passwordTitle')">
    <h2 class="profile-password__title">{{ t('profiles.passwordTitle') }}</h2>
    <v-text-field
      v-model="currentPassword"
      :label="t('accounts.currentPassword')"
      type="password"
      autocomplete="current-password"
      :maxlength="PASSWORD_MAX_LENGTH"
      required
      :disabled="accounts.busy.value"
    />
    <v-text-field
      v-model="newPassword"
      :label="t('accounts.newPassword')"
      type="password"
      autocomplete="new-password"
      :minlength="PASSWORD_MIN_LENGTH"
      :maxlength="PASSWORD_MAX_LENGTH"
      required
      :disabled="accounts.busy.value"
    />
    <PasswordStrengthMeter :password="newPassword" />
    <v-text-field
      v-model="confirmation"
      :label="t('accounts.confirmPassword')"
      type="password"
      autocomplete="new-password"
      :maxlength="PASSWORD_MAX_LENGTH"
      required
      :disabled="accounts.busy.value"
    />
    <p v-if="mismatch" class="profile-password__error" role="alert">{{ t('accounts.passwordMismatch') }}</p>
    <v-alert v-if="samePassword" type="warning" variant="tonal" class="profile-password__warning">
      <p>{{ t('accounts.samePasswordWarning') }}</p>
      <div class="profile-password__actions">
        <v-btn size="small" color="warning" variant="flat" :disabled="accounts.busy.value" @click="confirmSamePassword">
          {{ t('accounts.samePasswordConfirm') }}
        </v-btn>
        <v-btn size="small" variant="text" @click="samePassword = false">{{ t('accounts.cancel') }}</v-btn>
      </div>
    </v-alert>
    <p v-if="errorCode" class="profile-password__error" role="alert">{{ t(`accounts.errors.${errorCode}`) }}</p>
    <p class="profile-password__hint">{{ t('accounts.passwordHint') }}</p>
    <v-alert v-if="saved" type="success" variant="tonal" :text="t('profiles.passwordSaved')" class="profile-password__saved" />
    <v-btn color="primary" :loading="accounts.busy.value" :disabled="accounts.busy.value" @click="submit">
      {{ t('accounts.savePassword') }}
    </v-btn>
  </section>
</template>

<style scoped>
.profile-password {
  display: grid;
  gap: 12px;
  margin-top: 12px;
  padding-top: 16px;
  border-top: 1px solid var(--border-color-primary);
}

.profile-password__title {
  margin: 0;
  font-size: 1.1rem;
}

.profile-password__error {
  margin: 0;
  color: var(--text-danger);
}

.profile-password__hint {
  margin: 0;
  font-size: 0.875rem;
  opacity: 0.8;
}

.profile-password__warning,
.profile-password__saved {
  margin: 0;
}

.profile-password__actions {
  display: flex;
  gap: 8px;
  margin-top: 8px;
}
</style>