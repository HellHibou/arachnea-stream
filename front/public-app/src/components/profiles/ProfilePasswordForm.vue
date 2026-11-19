<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useAccountSession } from '@/composables/useAccountSession'
import { useI18n } from '@/i18n'
import { AccountApiError } from '@/services/accountApi'
import { PASSWORD_INPUT_MAX_LENGTH, PASSWORD_MIN_LENGTH } from '@/services/password'
import PasswordStrengthMeter from '@/components/accounts/PasswordStrengthMeter.vue'
import ParametersSection from '@/components/parameters/ParametersSection.vue'

const accounts = useAccountSession()
const { t } = useI18n()

const currentPassword = ref('')
const newPassword = ref('')
const confirmation = ref('')
const mismatch = ref(false)
const samePassword = ref(false)
const saved = ref(false)
const errorCode = ref<string | null>(null)
/** Warns only while a non-empty replacement is below the server's scalar-value minimum. */
const passwordTooShort = computed(() => newPassword.value.length > 0 && Array.from(newPassword.value).length < PASSWORD_MIN_LENGTH)

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
  <ParametersSection class="profile-password" :title="t('profiles.passwordTitle')" :aria-label="t('profiles.passwordTitle')">
    <v-text-field
      v-model="currentPassword"
      :label="t('accounts.currentPassword')"
      type="password"
      autocomplete="current-password"
      :maxlength="PASSWORD_INPUT_MAX_LENGTH"
      required
      :disabled="accounts.busy.value"
    />
    <v-text-field
      v-model="newPassword"
      :label="t('accounts.newPassword')"
      type="password"
      autocomplete="new-password"
      :minlength="PASSWORD_MIN_LENGTH"
      :maxlength="PASSWORD_INPUT_MAX_LENGTH"
      required
      :disabled="accounts.busy.value"
    />
    <PasswordStrengthMeter :password="newPassword" />
    <v-text-field
      v-model="confirmation"
      :label="t('accounts.confirmPassword')"
      type="password"
      autocomplete="new-password"
      :maxlength="PASSWORD_INPUT_MAX_LENGTH"
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
    <v-alert v-if="passwordTooShort" type="warning" variant="tonal" role="alert">{{ t('accounts.passwordHint') }}</v-alert>
    <v-alert v-if="saved" type="success" variant="tonal" :text="t('profiles.passwordSaved')" class="profile-password__saved" />
    <v-btn class="profile-password__submit" variant="flat" :loading="accounts.busy.value" :disabled="accounts.busy.value" @click="submit">
      {{ t('accounts.savePassword') }}
    </v-btn>
  </ParametersSection>
</template>

<style scoped>
.profile-password {
  display: grid;
  gap: 12px;
  padding-bottom: 16px;
}

.profile-password__error {
  margin: 0;
  color: var(--text-danger);
}

.profile-password__submit {
  justify-self: center;
  min-height: 40px;
  padding: 0 20px;
  border-radius: 999px;
  background: var(--bg-accent-primary);
  color: var(--text-primary);
  font: inherit;
  font-weight: 700;
}

.profile-password__submit:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
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
