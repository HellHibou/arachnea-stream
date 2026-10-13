<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useServiceMetadata } from '@/composables/useServiceMetadata'
import { useI18n } from '@/i18n'

/** Active source from the route; null selects the aggregated home. */
const props = defineProps<{ sourceId: string | null }>()
const { t } = useI18n()
const router = useRouter()
const { services, isLoading, errorMessage, load, refresh, getService } = useServiceMetadata()
const availableSources = computed(() => services.value.filter((service) => service.supportedQueries.includes('load_home')))
const activeService = computed(() => getService(props.sourceId))
const unavailable = computed(() => props.sourceId !== null && !isLoading.value && !availableSources.value.some((service) => service.id === props.sourceId))
const choices = computed(() => [
  { title: t('sourceHome.all'), value: '' },
  ...availableSources.value.map((service) => ({ title: service.title, value: service.id })),
  ...(unavailable.value ? [{ title: activeService.value?.title ?? props.sourceId ?? '', value: props.sourceId ?? '' }] : []),
])

/** Navigates without preloading homes. @param sourceId Exact source selected by the user. */
function selectSource(sourceId: string): void {
  void router.push(sourceId ? { name: 'source-home', params: { sourceId } } : { name: 'home' })
}
onMounted(() => { void load() })
</script>

<template>
  <section class="home-source-selector" :aria-label="t('sourceHome.select')">
    <v-select
      :model-value="props.sourceId ?? ''"
      :items="choices"
      :label="t('sourceHome.select')"
      :loading="isLoading"
      :disabled="isLoading"
      hide-details
      @update:model-value="selectSource"
    />
    <header v-if="props.sourceId !== null" class="home-source-selector__heading">
      <img v-if="activeService?.logo" class="home-source-selector__logo" :src="activeService.logo" alt="">
      <h1 class="home-source-selector__title">{{ activeService?.title ?? props.sourceId }}</h1>
    </header>
    <p v-if="unavailable" role="status">{{ t('sourceHome.unavailable') }}</p>
    <p v-if="errorMessage" role="alert">{{ errorMessage }}</p>
    <v-btn v-if="unavailable || errorMessage" @click="refresh">{{ t('sourceHome.retry') }}</v-btn>
  </section>
</template>

<style scoped>
.home-source-selector {
  position: relative;
  z-index: 1;
  display: grid;
  gap: 12px;
  margin: 16px;
  color: var(--text-primary);
}

.home-source-selector__heading {
  display: flex;
  gap: 12px;
  align-items: center;
}

.home-source-selector__logo {
  width: 48px;
  height: 48px;
  object-fit: contain;
}

.home-source-selector__title {
  margin: 0;
  font-size: 1.7rem;
}
</style>