<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from '@/i18n'
import { injectProfiles } from '@/composables/useProfiles'
import ProfileOnboarding from './ProfileOnboarding.vue'
import ProfileSwitcher from './ProfileSwitcher.vue'

const props = withDefaults(defineProps<{ open: boolean; mode?: 'select' | 'manage' }>(), { mode: 'manage' })
const emit = defineEmits<{ 'update:open': [value: boolean] }>()
const profiles = injectProfiles()
const { t } = useI18n()
const creating = ref(false)

/** Dialog title depending on the management/selection mode. */
const title = computed(() => props.mode === 'select' ? t('profiles.selectTitle') : t('profiles.manageTitle'))

/** Opens the reusable guide with a clean error state. */
function startCreation(): void {
  profiles.clearError()
  creating.value = true
}

/** Returns to the selector after a successful identity creation/hand-off. */
function close(): void {
  creating.value = false
  emit('update:open', false)
}
</script>

<template>
  <v-dialog :model-value="open" max-width="720" scrollable class="profile-dialog-overlay" @update:model-value="emit('update:open', $event)">
    <v-card class="profile-dialog">
      <v-card-title v-if="!creating && mode === 'manage'" class="profile-dialog__header">
        <h2 class="profile-dialog__title">{{ title }}</h2>
      </v-card-title>
      <v-card-text class="profile-dialog__content">
        <ProfileOnboarding v-if="creating" additional />
        <ProfileSwitcher
          v-else
          :manage="mode === 'manage'"
          :select-only="mode === 'select'"
          @create="startCreation"
          @selected="mode === 'select' && close()"
        >
          <template v-if="mode === 'select'" #actions>
            <v-btn variant="text" @click="close">{{ t('customHome.close') }}</v-btn>
          </template>
        </ProfileSwitcher>
      </v-card-text>
      <v-card-actions v-if="creating || mode === 'manage'" class="profile-dialog__actions">
        <v-btn variant="text" @click="close">{{ t('customHome.close') }}</v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>

<style scoped>
.profile-dialog {
  background: var(--bg-surface);
  color: var(--text-primary);
  font-family: inherit;
}

.profile-dialog__content :deep(.profile-switcher),
.profile-dialog__content :deep(.profile-onboarding) {
  width: 100%;
  margin: 0;
  padding: 8px 0;
}

.profile-dialog__actions {
  flex-wrap: wrap;
}

.profile-dialog__header {
  padding: 16px 24px 0;
}

.profile-dialog__title {
  position: relative;
  padding-inline-start: 12px;
  font-size: 1.16rem;
  font-weight: 800;
  letter-spacing: 0.01em;
  line-height: 1.2;
  white-space: normal;
}

.profile-dialog__title::before {
  position: absolute;
  top: 50%;
  inset-inline-start: 0;
  width: 4px;
  height: 18px;
  border-radius: 999px;
  background: var(--bg-accent-marker-primary);
  content: '';
  transform: translateY(-50%);
}

.profile-dialog-overlay :deep(.v-overlay__scrim) {
  backdrop-filter: var(--backdrop-filter-strong);
  opacity: 1;
  background: rgb(0 0 0 / 45%);
}
</style>
