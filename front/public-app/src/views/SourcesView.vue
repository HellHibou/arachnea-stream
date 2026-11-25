<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch } from 'vue'

import SourceCard from '@/components/sources/SourceCard.vue'
import { useServiceMetadata } from '@/composables/useServiceMetadata'
import { APP_TITLE } from '@/constants'
import { useI18n } from '@/i18n'

/** Internationalization utilities. */
const { t } = useI18n()
/** Session-scoped service metadata registry. */
const metadata = useServiceMetadata()

/** Sources advertising a home catalog, matching the navigation source group. */
const sources = computed(() => metadata.services.value.filter((service) => service.supportedQueries.includes('load_home')))

/** Document title for the all-sources overview. */
const pageTitle = computed(() => `${t('navigation.sources')} - ${APP_TITLE}`)

watch(pageTitle, (title) => {
  document.title = title
}, { immediate: true })

onUnmounted(() => {
  document.title = APP_TITLE
})

onMounted(() => {
  void metadata.load()
})

/** Retries service metadata discovery after a failed load. */
function retrySources(): void {
  void metadata.refresh()
}
</script>

<template>
  <main class="sources-view">
    <section class="sources-view__shell">
      <header class="sources-view__header">
        <h1 class="sources-view__title">{{ t('navigation.sources') }}</h1>
      </header>

      <p v-if="metadata.isLoading.value && sources.length === 0" class="sources-view__status" role="status">
        {{ t('sourceHome.loading') }}
      </p>

      <article v-else-if="metadata.errorMessage.value && sources.length === 0" class="sources-view__status">
        <p role="alert">{{ metadata.errorMessage.value }}</p>
        <v-btn @click="retrySources">{{ t('sourceHome.retry') }}</v-btn>
      </article>

      <p v-else-if="sources.length === 0" class="sources-view__status" role="status">
        {{ t('sourceHome.noSources') }}
      </p>

      <ul v-else class="sources-view__grid">
        <li v-for="source in sources" :key="source.id">
          <SourceCard :source="source" />
        </li>
      </ul>
    </section>
  </main>
</template>

<style scoped>
.sources-view {
  position: relative;
  min-height: 100vh;
  min-height: 100dvh;
  color: var(--text-primary);
}

.sources-view__shell {
  position: relative;
  z-index: 1;
  width: calc(100% - 32px);
  max-width: 1440px;
  margin: 0 auto;
  padding: 16px 0 32px;
}

.sources-view__header {
  margin-bottom: 24px;
}

.sources-view__title {
  margin: 0;
  font-size: 1.7rem;
}

.sources-view__status {
  display: grid;
  width: fit-content;
  max-width: min(100%, 620px);
  margin: 0 auto;
  padding: 36px 24px;
  border: 1px solid var(--border-color-primary);
  border-radius: var(--radius);
  background: var(--bg-surface);
  box-shadow: var(--inset-light);
  color: var(--text-secondary);
  gap: 16px;
  justify-items: center;
  text-align: center;
}

.sources-view__status p {
  margin: 0;
}

.sources-view__grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 16px;
  margin: 0;
  padding: 0;
  list-style: none;
}

@media (max-width: 960px) {
  .sources-view__shell {
    width: min(100%, calc(100% - 20px));
    padding: 12px 0 24px;
  }

  .sources-view__grid {
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 12px;
  }
}
</style>
