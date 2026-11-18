<script setup lang="ts">
import { computed, useId } from 'vue'
import { useI18n } from '@/i18n'
import type { AvatarCatalogue } from '@/services/profileApi'
import ProfileAvatar from './ProfileAvatar.vue'

/** Reusable identity fields; validation is local and the parent owns persistence. */
const props = withDefaults(defineProps<{
  name: string
  avatarId: string
  catalogue: AvatarCatalogue | null
  busy?: boolean
  errorCode?: string | null
}>(), { busy: false, errorCode: null })

const emit = defineEmits<{
  'update:name': [value: string]
  'update:avatarId': [value: string]
  submit: []
}>()

const { t } = useI18n()
const componentId = useId()
const nameInputId = `${componentId}-name`

const trimmedName = computed(() => props.name.trim())
const nameValid = computed(() => trimmedName.value.length > 0 && trimmedName.value.length <= 64)

/** Emits the current identity once the local name constraint is satisfied. */
function submit(): void {
  if (nameValid.value && props.avatarId) emit('submit')
}

/** @param event Native name input event. */
function updateName(event: Event): void {
  emit('update:name', (event.target as HTMLInputElement).value)
}

/** @param event Change event from the avatar radio list. */
function updateAvatar(event: Event): void {
  emit('update:avatarId', (event.target as HTMLInputElement).value)
}
</script>

<template>
  <form class="profile-form" @submit.prevent="submit">
    <label class="profile-form__field" :for="nameInputId">
      <span>{{ t('profiles.name') }}</span>
      <input
        :id="nameInputId"
        class="profile-form__input"
        :value="name"
        maxlength="64"
        autocomplete="off"
        required
        :disabled="busy"
        @input="updateName"
      >
    </label>
    <p v-if="!nameValid && name.length > 0" class="profile-form__error" role="alert">{{ t('profiles.nameInvalid') }}</p>

    <fieldset class="profile-form__avatars" :disabled="busy">
      <legend>{{ t('profiles.avatar') }}</legend>
      <label
        v-for="avatar in catalogue?.avatars ?? []"
        :key="avatar.avatar_id"
        class="profile-form__avatar"
        :class="{ 'profile-form__avatar--selected': avatar.avatar_id === avatarId }"
        :title="avatar.avatar_id"
      >
        <input
          class="profile-form__avatar-input"
          type="radio"
          name="profile-avatar"
          :value="avatar.avatar_id"
          :checked="avatar.avatar_id === avatarId"
          @change="updateAvatar"
        >
        <ProfileAvatar :avatar-id="avatar.avatar_id" :catalogue="catalogue" :name="name" :size="56" />
      </label>
    </fieldset>

    <p v-if="errorCode" class="profile-form__error" role="alert">{{ t(`profiles.errors.${errorCode}`) }}</p>
    <slot />
    <button class="profile-form__submit" type="submit" :disabled="busy || !nameValid || !avatarId">
      {{ busy ? t('profiles.saving') : t('profiles.create') }}
    </button>
  </form>
</template>

<style scoped>
.profile-form {
  display: grid;
  gap: 16px;
  width: min(520px, 100%);
}

.profile-form__field {
  display: grid;
  gap: 6px;
  font-weight: 600;
}

.profile-form__input {
  min-height: 40px;
  padding: 8px 14px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  color: var(--text-primary);
  font: inherit;
}

.profile-form__input:focus-visible,
.profile-form__submit:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.profile-form__avatars {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin: 0;
  padding: 0;
  border: 0;
}

.profile-form__avatars legend {
  margin-bottom: 8px;
  font-weight: 600;
}

.profile-form__avatar {
  position: relative;
  display: inline-flex;
  padding: 3px;
  border: 2px solid transparent;
  border-radius: 50%;
  cursor: pointer;
}

.profile-form__avatar--selected {
  border-color: var(--color-primary);
}

.profile-form__avatar-input {
  position: absolute;
  width: 1px;
  height: 1px;
  opacity: 0;
}

.profile-form__avatar-input:focus-visible + :deep(.profile-avatar) {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.profile-form__error {
  margin: 0;
  color: var(--text-danger);
}

.profile-form__submit {
  justify-self: start;
  padding: 10px 18px;
  border: 0;
  border-radius: 999px;
  background: var(--color-primary);
  color: var(--text-primary);
  font: inherit;
  cursor: pointer;
}

.profile-form__submit:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
