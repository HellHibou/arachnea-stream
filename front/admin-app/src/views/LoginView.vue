<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'

import { useAdminApi } from '@/composables/useAdminApi'
import { useI18n } from '@/i18n'

const router = useRouter()
const { t } = useI18n()
const { login, isLoading, error } = useAdminApi()

const password = ref('')
const loginError = ref<string | null>(null)

async function handleLogin(): Promise<void> {
  loginError.value = null
  if (!password.value) {
    loginError.value = t('auth.invalidPassword')
    return
  }

  const success = await login(password.value)
  if (success) {
    router.push({ name: 'services' })
  } else {
    loginError.value = error.value?.message ?? t('auth.loginFailed')
    password.value = ''
  }
}
</script>

<template>
  <v-container class="fill-height" fluid>
    <v-row align="center" justify="center">
      <v-col cols="12" sm="8" md="6" lg="4">
        <v-card class="elevation-12">
          <v-card-title class="text-center">
            <h1 class="text-h5">{{ t('app.title') }}</h1>
            <p class="text-subtitle-1 text-medium-emphasis">
              {{ t('auth.loginRequired') }}
            </p>
          </v-card-title>

          <v-card-text>
            <p class="text-body-2 mb-4">
              {{ t('auth.loginDescription') }}
            </p>

            <v-form @submit.prevent="handleLogin">
              <v-text-field
                v-model="password"
                :label="t('auth.password')"
                :placeholder="t('auth.passwordPlaceholder')"
                type="password"
                variant="outlined"
                :error-messages="loginError"
                :disabled="isLoading"
                autofocus
              />

              <v-btn
                type="submit"
                color="primary"
                block
                size="large"
                :loading="isLoading"
              >
                {{ t('auth.login') }}
              </v-btn>
            </v-form>
          </v-card-text>
        </v-card>
      </v-col>
    </v-row>
  </v-container>
</template>
