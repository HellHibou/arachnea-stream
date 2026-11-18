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
  <v-dialog :model-value="open" max-width="720" scrollable @update:model-value="emit('update:open', $event)">
    <v-card class="profile-dialog">
      <v-card-title v-if="!creating && mode === 'manage'">{{ title }}</v-card-title>
      <v-card-text>
        <ProfileOnboarding v-if="creating" additional />
        <ProfileSwitcher
          v-else
          :manage="mode === 'manage'"
          :select-only="mode === 'select'"
          @create="startCreation"
          @selected="mode === 'select' && close()"
        />
      </v-card-text>
      <v-card-actions>
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
</style>