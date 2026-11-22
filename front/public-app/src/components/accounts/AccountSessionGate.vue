<script setup lang="ts">
import { ref, watch } from 'vue'
import router from '@/router'
import { useAccountSession, useAccountSessionMonitor } from '@/composables/useAccountSession'
import { useServiceMetadata } from '@/composables/useServiceMetadata'
import { useStorage } from '@/services/storage'
import { loadVideoSourceAllowlist } from '@/services/videoSourceAllowlist'
import { applyTheme, findThemePreset } from '@/services/theme'
import { sessionGeneration } from '@/services/sessionEvents'
import { t } from '@/i18n'
import { resolveAppPath } from '@/services/baseUrl'
import { isDesktopApp, openAdministrationWindow } from '@/services/rustify/desktop'
import AccountCredentialsForm from './AccountCredentialsForm.vue'
import ProfileSessionGate from '@/components/profiles/ProfileSessionGate.vue'

const accounts = useAccountSession()
const contentReady = ref(false)
const contentKey = ref(0)
const startupError = ref(false)
const adminOpenError = ref(false)
useAccountSessionMonitor()
/** Initializes content only after an unrestricted session has been confirmed. */
async function prepareContent(): Promise<void> {
  const generation = sessionGeneration()
  contentReady.value = false
  startupError.value = false
  if (!accounts.ready.value) return
  try {
    await Promise.all([useStorage().initStorage(), loadVideoSourceAllowlist()])
    applyTheme(findThemePreset(useStorage().getParameters().theme.value))
    await useServiceMetadata().load()
    if (generation !== sessionGeneration() || !accounts.ready.value) return
    await router.replace({ path: router.currentRoute.value.fullPath, force: true })
    contentKey.value = generation
    contentReady.value = true
  } catch { if (generation === sessionGeneration()) startupError.value = true }
}
watch([accounts.ready, () => accounts.session.value?.account?.user_id], () => { void prepareContent() }, { immediate: true })
/** Sends form credentials and leaves failed drafts mounted for correction. @param login Login identifier. @param password Existing password. @param replacement New password for mandatory replacement. */
async function submit(login: string, password: string, replacement: string): Promise<void> {
  try {
    if (accounts.session.value?.account?.must_change_password) await accounts.changePassword(password, replacement)
    else await accounts.login(login, password)
  } catch { /* The composable exposes a localized error code. */ }
}
/** Revokes the server session; errors leave the current state visible. */
async function logout(): Promise<void> { try { await accounts.logout() } catch { /* Visible shared error. */ } }
/** Opens the independent administrator UI so the first public account can be created. */
async function openAdmin(): Promise<void> {
  adminOpenError.value = false
  try {
    if (isDesktopApp()) await openAdministrationWindow()
    else window.open(resolveAppPath('admin/'), '_blank', 'noopener,noreferrer')
  } catch { adminOpenError.value = true }
}
</script>

<template>
  <div v-if="!accounts.restored.value || (accounts.ready.value && !contentReady && !startupError)" class="account-gate" role="status" />
  <template v-else-if="accounts.ready.value && contentReady">
    <ProfileSessionGate :key="contentKey" />
    <p v-if="accounts.errorCode.value" class="session-warning" role="alert">{{ t(`accounts.errors.${accounts.errorCode.value}`) }}</p>
    <p v-if="!accounts.session.value?.persistent" class="session-warning" role="status">{{ t('accounts.memoryOnly') }}</p>
  </template>
  <div v-else-if="accounts.ready.value" class="account-gate">
    <p role="alert">{{ t('accounts.errors.storage_unavailable') }}</p>
    <v-btn @click="prepareContent">{{ t('accounts.retry') }}</v-btn>
    <v-btn @click="logout">{{ t('accounts.signOut') }}</v-btn>
  </div>
  <div v-else class="account-gate">
    <AccountCredentialsForm :key="accounts.session.value?.account?.must_change_password ? 'change' : 'login'"
      :change-password="Boolean(accounts.session.value?.account?.must_change_password)" :busy="accounts.busy.value" :error-code="accounts.errorCode.value" @submit="submit" />
    <v-btn v-if="accounts.session.value?.authenticated" :disabled="accounts.busy.value" @click="logout">{{ t('accounts.signOut') }}</v-btn>
    <v-btn variant="text" @click="openAdmin">{{ t('accounts.administration') }}</v-btn>
    <p v-if="adminOpenError" role="alert">{{ t('accounts.adminOpenError') }}</p>
    <v-btn v-if="accounts.errorCode.value === 'storage_unavailable'" @click="accounts.restore">{{ t('accounts.retry') }}</v-btn>
  </div>
</template>

<style scoped>
.account-gate {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-height: 100dvh;
  padding: 24px;
}

.session-warning {
  position: fixed;
  right: 0;
  bottom: 0;
  z-index: 50;
  padding: 12px;
  background: var(--bg-surface);
}
</style>
