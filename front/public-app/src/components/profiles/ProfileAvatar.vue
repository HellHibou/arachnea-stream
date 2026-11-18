<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { AvatarCatalogue } from '@/services/profileApi'

/** Avatar identity, authenticated catalogue and display fallback. */
const props = withDefaults(defineProps<{
  avatarId: string
  catalogue: AvatarCatalogue | null
  name?: string
  size?: number
}>(), { name: '', size: 48 })

const failed = ref(false)

/** Resolves the stored avatar, then the catalogue fallback, keeping a removed identity displayable. */
const entry = computed(() => {
  const catalogue = props.catalogue
  if (!catalogue) return null
  return catalogue.avatars.find((avatar) => avatar.avatar_id === props.avatarId)
    ?? catalogue.avatars.find((avatar) => avatar.avatar_id === catalogue.fallback_avatar_id)
    ?? null
})

/** First visible character used when no catalogue image can be displayed. */
const initial = computed(() => props.name.trim().charAt(0).toUpperCase() || '?')

watch(() => [props.avatarId, props.catalogue], () => { failed.value = false })
</script>

<template>
  <span class="profile-avatar" :style="{ '--profile-avatar-size': `${size}px` }" aria-hidden="true">
    <img v-if="entry && !failed" class="profile-avatar__image" :src="entry.url" :alt="''" @error="failed = true">
    <span v-else class="profile-avatar__initial">{{ initial }}</span>
  </span>
</template>

<style scoped>
.profile-avatar {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: var(--profile-avatar-size);
  height: var(--profile-avatar-size);
  overflow: hidden;
  border: 1px solid var(--border-color-primary);
  border-radius: 50%;
  background: var(--bg-surface);
  color: var(--text-primary);
  font-weight: 700;
}

.profile-avatar__image {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.profile-avatar__initial {
  font-size: calc(var(--profile-avatar-size) * 0.42);
  line-height: 1;
}
</style>
