<script setup lang="ts">
import { computed, nextTick, shallowRef, useTemplateRef, watch } from 'vue'
import type { ComponentPublicInstance } from 'vue'
import { RouterView, useRoute, useRouter, type RouteLocationRaw } from 'vue-router'
import { type Parameters, useStorage } from '@/services/storage'
import {
  buildSearchRouteQuery,
  readSearchRouteQuery,
} from '@/router/searchQuery'
import {
  encodeEntryRoutePayload,
  encodeLiveRoutePayload,
} from '@/router/routePayloads'
import type { BackgroundMediaCandidate, MediaSelectionTarget } from '@/types/media'
import type { HomeCategory } from '@/types/home'
import Background from './components/Background.vue'
import MainBar from './components/MainBar.vue'
import { useSearchFilterOptions } from '@/composables/useSearchFilterOptions'

const route = useRoute()
const router = useRouter()
const mainBarRef =
  useTemplateRef<ComponentPublicInstance<{ focusSearchInput: () => void }>>('mainBar')
const routeSearchState = computed(() => readSearchRouteQuery(route.query))
const searchQuery = shallowRef('')
const selectedMediaTypes = shallowRef<string[]>([])
const selectedThemes = shallowRef<string[]>([])

const isParametersVisible = shallowRef(false)
const isSearchVisible = shallowRef(false)
const parameters: Parameters = useStorage().getParameters()
const { mediaTypeFilterOptions, themeFilterOptions } = useSearchFilterOptions()

const catalogBackgroundMediaItems = shallowRef<BackgroundMediaCandidate[]>([])

/**
 * Indicates whether the generic back button should be visible for the current route.
 */
const showBackButton = computed(() => route.name !== 'home')

/**
 * Indicates whether the app-level background layer should be rendered.
 */
const shouldShowAppBackground = computed(
  () => route.name !== 'entry-details' && route.name !== 'lives' && route.name !== 'live-details',
)

watch(
  () => [route.name, JSON.stringify(routeSearchState.value)] as const,
  () => {
    if (route.name !== 'search') {
      return
    }

    searchQuery.value = routeSearchState.value.query
    selectedMediaTypes.value = [...routeSearchState.value.mediaTypes]
    selectedThemes.value = [...routeSearchState.value.themes]
  },
  { immediate: true },
)

/**
 * Toggles the visibility of the parameters popover.
 */
function toggleParameters() {
  isParametersVisible.value = !isParametersVisible.value
}

/**
 * Closes the parameters popover.
 */
function closeParameters() {
  isParametersVisible.value = false
}

/**
 * Toggles the visibility of the search bar and focuses the input when shown.
 */
function toggleSearch() {
  isSearchVisible.value = !isSearchVisible.value
  if (isSearchVisible.value) {
    nextTick(() => {
      mainBarRef.value?.focusSearchInput()
    })
  }
}

/**
 * Returns the background images that should be shown behind catalog screens.
 */
const catalogScreenBackgroundMediaItems = computed(() => {
  if (!parameters.useCatalogBannersAsBackground.value) {
    return []
  }

  if (route.name !== 'home' && route.name !== 'category') {
    return []
  }

  return catalogBackgroundMediaItems.value
})

/**
 * Stores the current banner background image candidates exposed by catalog screens.
 *
 * @param mediaItems Banner background image candidates collected from the active catalog payload.
 */
function handleCatalogBackgroundMediaItemsUpdate(mediaItems: BackgroundMediaCandidate[]) {
  catalogBackgroundMediaItems.value = mediaItems
}

/**
 * Navigates to the requested route without awaiting the Vue Router promise in UI handlers.
 *
 * @param target Route location passed to Vue Router.
 */
function navigateTo(target: RouteLocationRaw) {
  void router.push(target)
}

/**
 * Submits the toolbar search and exposes the query and filters in the URL.
 */
function handleSearchSubmit() {
  navigateTo({
    name: 'search',
    query: buildSearchRouteQuery({
      query: searchQuery.value,
      mediaTypes: selectedMediaTypes.value,
      themes: selectedThemes.value,
    }),
  })
}

/**
 * Opens the detail route for a selected catalog/search entry.
 *
 * @param item Entry target selected from a rendered media card or banner.
 */
function handleSelectedItem(item: MediaSelectionTarget) {
  if (!item.source || !item.entryUrl) {
    return
  }

  navigateTo({
    name: 'entry-details',
    params: {
      encodedEntry: encodeEntryRoutePayload({
        source: item.source,
        entryUrl: item.entryUrl,
      }),
    },
  })
}

/**
 * Opens the route for one aggregated category.
 *
 * @param category Category selected from the home/category strip.
 */
function handleSelectedCategory(category: HomeCategory) {
  navigateTo({
    name: 'category',
    params: {
      categoryKey: category.mergeKey,
    },
  })
}

/**
 * Opens the route for one selected live stream.
 *
 * @param item Live target selected from the live list.
 */
function handleSelectedLive(item: MediaSelectionTarget) {
  if (!item.source || !item.entryUrl) {
    return
  }

  navigateTo({
    name: 'live-details',
    params: {
      encodedLive: encodeLiveRoutePayload({
        v: 1,
        source: item.source,
        channel: item.entryUrl,
      }),
    },
  })
}

/**
 * Navigates to the home route.
 */
function handleGoHome() {
  navigateTo({ name: 'home' })
}

/**
 * Navigates to the live list route.
 */
function handleGoLives() {
  navigateTo({ name: 'lives' })
}

/**
 * Uses browser history when Vue Router knows a previous location, otherwise returns home.
 */
function handleBackNavigation() {
  if (typeof window.history.state?.back === 'string') {
    router.back()
    return
  }

  handleGoHome()
}
</script>

<template>
  <div class="app-shell">
    <Background
      v-if="shouldShowAppBackground"
      :is-animated="parameters.isBackgroundAnimated.value"
      :media-items="catalogScreenBackgroundMediaItems"
      :image-fit="parameters.backgroundImageFit.value"
    />

    <MainBar
      ref="mainBar"
      v-model="searchQuery"
      :media-type-options="mediaTypeFilterOptions"
      :selected-media-types="selectedMediaTypes"
      :theme-options="themeFilterOptions"
      :selected-themes="selectedThemes"
      :show-back-button="showBackButton"
      :is-parameters-visible="isParametersVisible"
      :is-search-visible="isSearchVisible"
      @submit="handleSearchSubmit"
      @update:selected-media-types="selectedMediaTypes = $event"
      @update:selected-themes="selectedThemes = $event"
      @back="handleBackNavigation"
      @home="handleGoHome"
      @lives="handleGoLives"
      @toggle-parameters="toggleParameters"
      @close-parameters="closeParameters"
      @toggle-search="toggleSearch"
    />

    <main class="app-content">
      <RouterView v-slot="{ Component }">
        <component
          :is="Component"
          @select-item="handleSelectedItem"
          @select-category="handleSelectedCategory"
          @select-live="handleSelectedLive"
          @update:background-media-items="handleCatalogBackgroundMediaItemsUpdate"
        />
      </RouterView>
    </main>
  </div>
</template>

<style scoped>
.app-shell {
  min-height: 100vh;
  min-height: 100dvh;
  padding: 0 16px 32px;
  background: var(--bg-transparent);
}

.app-content {
  display: grid;
  min-width: 0;
  min-height: 0;
}

.app-content :deep(.media-search) {
  min-height: calc(100vh - 124px);
  min-height: calc(100dvh - 124px);
}

@media (max-width: 640px) {
  .app-shell {
    padding: 0 10px 24px;
  }

  .app-content :deep(.media-search) {
    min-height: calc(100vh - 110px);
    min-height: calc(100dvh - 110px);
  }
}
</style>
