<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from '@/i18n'
import { createOperationId } from '@/services/operationId'
import { PASSWORD_MAX_LENGTH, PASSWORD_MIN_LENGTH } from '@/services/password'
import PasswordStrengthMeter from './PasswordStrengthMeter.vue'

/** Create/reset mode, fixed reset target and busy state; credentials stay in memory only. */
const props = withDefaults(defineProps<{
  /** Whether the dialog is shown. */
  modelValue?: boolean
  /** Fixed account login in reset mode; empty in creation mode. */
  resetLogin?: string
  /** Prevents closing and submitting while an account operation runs. */
  busy?: boolean
  /** Stable error code from the last failed account operation. */
  errorCode?: string | null
}>(), { modelValue: false, resetLogin: '', busy: false, errorCode: null })

const emit = defineEmits<{
  close: []
  submit: [operationId: string, login: string, password: string, mustChangePassword: boolean]
}>()

const { t } = useI18n()
const login = ref('')
const password = ref('')
const mustChangePassword = ref(true)
const showPassword = ref(false)
const operationId = ref(createOperationId())
const activeResetLogin = ref('')

const isReset = computed(() => Boolean(activeResetLogin.value))
const canSave = computed(() => (isReset.value || login.value.trim().length >= 3) && Array.from(password.value).length >= PASSWORD_MIN_LENGTH)

/** Regenerates the retry identity whenever the submitted draft changes. */
watch([login, password, mustChangePassword], () => { operationId.value = createOperationId() })

/** Starts every opening from an empty draft and freezes its create/reset mode. */
watch(() => props.modelValue, (open) => {
  if (!open) return
  activeResetLogin.value = props.resetLogin
  resetForm()
})

/** Clears the in-memory credential draft. */
function resetForm(): void {
  login.value = ''
  password.value = ''
  mustChangePassword.value = true
  showPassword.value = false
  operationId.value = createOperationId()
}

/** Closes the dialog unless a submitted operation is still running. */
function close(): void {
  if (!props.busy) emit('close')
}

/** Emits the credential draft with a stable retry UUID, never persisting the secret. */
function submit(): void {
  if (!canSave.value || props.busy) return
  emit(
    'submit',
    operationId.value,
    isReset.value ? activeResetLogin.value : login.value.trim(),
    password.value,
    isReset.value ? true : mustChangePassword.value,
  )
}
</script>

<template>
  <v-dialog :model-value="modelValue" max-width="640" :persistent="busy" @update:model-value="!$event && close()">
    <v-card>
      <v-card-title>{{ t(isReset ? 'users.reset' : 'users.create') }}</v-card-title>
      <v-card-text>
        <v-alert v-if="errorCode" type="error" variant="tonal" class="mb-4" role="alert">{{ t(`users.errors.${errorCode}`) }}</v-alert>
        <v-form @submit.prevent="submit">
          <v-text-field
            v-if="isReset"
            :model-value="activeResetLogin"
            :label="t('users.login')"
            variant="outlined"
            disabled
          />
          <v-text-field
            v-else
            v-model="login"
            :label="t('users.login')"
            variant="outlined"
            autocomplete="off"
            minlength="3"
            maxlength="64"
            required
            :disabled="busy"
          />

          <v-text-field
            v-model="password"
            :label="t('users.temporaryPassword')"
            :type="showPassword ? 'text' : 'password'"
            variant="outlined"
            autocomplete="new-password"
            :minlength="PASSWORD_MIN_LENGTH"
            :maxlength="PASSWORD_MAX_LENGTH"
            required
            :disabled="busy"
            class="mt-2"
          >
            <template #append-inner>
              <v-icon
                :icon="showPassword ? 'mdi-eye-off' : 'mdi-eye'"
                :aria-label="showPassword ? t('users.hidePassword') : t('users.showPassword')"
                role="button"
                tabindex="0"
                @click="showPassword = !showPassword"
              />
            </template>
          </v-text-field>
          <PasswordStrengthMeter :password="password" />

          <v-checkbox
            v-if="!isReset"
            v-model="mustChangePassword"
            :label="t('users.mustChangeOption')"
            :disabled="busy"
            hide-details
            density="compact"
            class="mb-2"
          />

          <p class="text-body-2">{{ t('users.passwordHint') }}</p>
        </v-form>
      </v-card-text>
      <v-card-actions>
        <v-spacer />
        <v-btn variant="text" :disabled="busy" @click="close">{{ t('users.cancel') }}</v-btn>
        <v-btn
          color="primary"
          variant="elevated"
          :loading="busy"
          :disabled="busy || !canSave"
          @click="submit"
        >
          {{ t(isReset ? 'users.reset' : 'users.create') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>
