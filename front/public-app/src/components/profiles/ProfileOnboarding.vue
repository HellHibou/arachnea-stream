<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from '@/i18n'
import { injectProfiles } from '@/composables/useProfiles'
import { defaultThemeKey } from '@/services/theme'
import ProfileForm from './ProfileForm.vue'
import ProfileCommonPreferences from './ProfileCommonPreferences.vue'

/** @property additional - Whether this guide creates an additional profile instead of the first one. */
const props = withDefaults(defineProps<{ additional?: boolean }>(), { additional: false })
const profiles = injectProfiles()
const { t } = useI18n()

const step = ref<'identity' | 'service'>('identity')
const name = ref('')
const avatarId = ref('')
const language = ref<string | null>(null)
const theme = ref(defaultThemeKey)
const createdProfileId = ref<string | null>(null)

/** Catalogue fallback keeps the form submittable when no default was selected. */
const fallbackAvatarId = computed(() => profiles.avatars.value?.fallback_avatar_id ?? '')
watch(() => profiles.avatars.value, (catalogue) => {
  if (!avatarId.value) avatarId.value = catalogue?.fallback_avatar_id ?? catalogue?.avatars[0]?.avatar_id ?? ''
}, { immediate: true })
watch(fallbackAvatarId, (value) => { if (!avatarId.value) avatarId.value = value })

/** Creates the common profile, then exposes the streaming first-use step. */
async function create(): Promise<void> {
  const result = await profiles.createProfile({ name: name.value, avatarId: avatarId.value, language: language.value, theme: theme.value })
  if (!result) return
  createdProfileId.value = result.profileId
  step.value = 'service'
}

/** Activates the created profile, initializing streaming at this first service use. */
async function start(): Promise<void> {
  if (!createdProfileId.value) return
  await profiles.select(createdProfileId.value)
}
</script>

<template>
  <section class="profile-onboarding" :aria-label="props.additional ? t('profiles.createTitle') : t('profiles.onboardingTitle')">
    <header class="profile-onboarding__header">
      <h1>{{ props.additional ? t('profiles.createTitle') : t('profiles.onboardingTitle') }}</h1>
      <p>{{ step === 'identity' ? t('profiles.identityIntro') : t('profiles.serviceIntro') }}</p>
    </header>

    <template v-if="step === 'identity'">
      <ProfileForm
        :name="name"
        :avatar-id="avatarId"
        :catalogue="profiles.avatars.value"
        :busy="profiles.busy.value"
        :error-code="profiles.errorCode.value"
        @update:name="name = $event"
        @update:avatarId="avatarId = $event"
        @submit="create"
      >
        <ProfileCommonPreferences
          :language="language"
          :theme="theme"
          :busy="profiles.busy.value"
          @update:language="language = $event"
          @update:theme="theme = $event"
        />
      </ProfileForm>
    </template>

    <template v-else>
      <p class="profile-onboarding__service">{{ t('profiles.serviceBody') }}</p>
      <p v-if="profiles.errorCode.value" class="profile-onboarding__error" role="alert">{{ t(`profiles.errors.${profiles.errorCode.value}`) }}</p>
      <button class="profile-onboarding__start" type="button" :disabled="profiles.busy.value" @click="start">
        {{ profiles.busy.value ? t('profiles.preparing') : t('profiles.start') }}
      </button>
    </template>
  </section>
</template>

<style scoped>
.profile-onboarding {
  display: grid;
  gap: 24px;
  width: min(560px, 100%);
  margin: auto;
  padding: 32px 24px;
}

.profile-onboarding__header {
  display: grid;
  gap: 8px;
}

.profile-onboarding__header h1 {
  margin: 0;
  font-size: 1.6rem;
}

.profile-onboarding__header p,
.profile-onboarding__service,
.profile-onboarding__error {
  margin: 0;
}

.profile-onboarding__error {
  color: var(--text-danger);
}

.profile-onboarding__start {
  justify-self: start;
  padding: 10px 18px;
  border: 0;
  border-radius: 999px;
  background: var(--color-primary);
  color: var(--text-primary);
  font: inherit;
  cursor: pointer;
}

.profile-onboarding__start:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.profile-onboarding__start:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
