<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from 'vue'
import { useRouter } from 'vue-router'

import { categoryMediaTarget, mediaRoute } from '@/services/mediaRouting'
import type { MediaCardCollectionMode } from '@/types/media'
import type { HomeCategory } from '@/types/home'
import { useI18n } from '@/i18n'
import HomePinButton from '@/components/home/HomePinButton.vue'
import HomeSectionCollectionToggle from '@/components/home/HomeSectionCollectionToggle.vue'

/**
 * Props accepted by the clickable category strip.
 */
interface Props {
   /**
    * Categories exposed by the backend catalog payload.
    */
   categories: HomeCategory[]
   /** Edition-only add action, also available for custom shortcuts. @default false */
   showAddButton?: boolean
}

/** Component props with edition controls hidden by default. */
const props = withDefaults(defineProps<Props>(), { showAddButton: false })
const { t } = useI18n()

const emit = defineEmits<{
  /** Emitted when a category is selected. */
   'select-category': [category: HomeCategory]
   'add-categories': [categories: HomeCategory[]]
}>()

/** Vue Router instance used to resolve category links. */
const router = useRouter()

/** Layout of the category list, collapsed to one row by default. */
const mode = ref<MediaCardCollectionMode>('single-row')
/** Template reference to the horizontal scroll viewport. */
const viewportRef = useTemplateRef<HTMLDivElement>('viewport')
/** Whether the viewport can scroll towards the first categories. */
const canScrollLeft = ref(false)
/** Whether the viewport can scroll towards the last categories. */
const canScrollRight = ref(false)
/** Whether navigation buttons are needed for the current layout. */
const showScrollControls = computed(() =>
  mode.value === 'single-row' && (canScrollLeft.value || canScrollRight.value),
)
/** Resize observer keeping navigation availability in sync with the viewport size. */
let resizeObserver: ResizeObserver | null = null

/** Switches between the single-row and multi-row category layouts. */
function toggleMode(): void {
  mode.value = mode.value === 'single-row' ? 'grid' : 'single-row'
}

/** Updates horizontal navigation availability from the viewport scroll position. */
function updateScrollState(): void {
  const viewport = viewportRef.value

  if (!viewport || mode.value !== 'single-row') {
    canScrollLeft.value = false
    canScrollRight.value = false
    return
  }

  const maxScrollLeft = Math.max(viewport.scrollWidth - viewport.clientWidth, 0)
  canScrollLeft.value = viewport.scrollLeft > 1
  canScrollRight.value = viewport.scrollLeft < maxScrollLeft - 1
}

/**
 * Scrolls the single-row viewport by roughly one visible page.
 *
 * @param direction Horizontal direction applied to the category viewport.
 */
function scrollRow(direction: 'left' | 'right'): void {
  const viewport = viewportRef.value

  if (!viewport) return

  const offset = Math.max(viewport.clientWidth * 0.82, 240)
  viewport.scrollBy({
    left: direction === 'right' ? offset : -offset,
    behavior: 'smooth',
  })
}

onMounted(() => {
  void nextTick(() => {
    updateScrollState()

    if (!viewportRef.value || typeof ResizeObserver === 'undefined') return

    resizeObserver = new ResizeObserver(() => updateScrollState())
    resizeObserver.observe(viewportRef.value)
  })
})

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  resizeObserver = null
})

watch([mode, () => props.categories.length], () => {
  updateScrollState()
}, { flush: 'post' })

/**
 * Resolves the internal category route targeted by a category tag.
 *
 * @param category Category from the strip.
 * @returns Browser href for the category route.
 */
function categoryHref(category: HomeCategory): string | undefined {
  const destination = mediaRoute(categoryMediaTarget(category))
  return destination ? router.resolve(destination).href : undefined
}

/**
 * Emits the selected category so the parent can load its aggregated catalog.
 *
 * @param category Category selected from the strip.
 * @param event Mouse event, preserving native modified link navigation.
 */
function handleCategorySelect(category: HomeCategory, event: MouseEvent) {
   if (event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return
   event.preventDefault()
   emit('select-category', category)
}
</script>

<template>
   <section class="home-category-strip" aria-labelledby="home-category-strip-title">
     <header class="home-category-strip__header">
       <HomeSectionCollectionToggle :mode="mode" @toggle="toggleMode" />
       <h2 id="home-category-strip-title" class="home-category-strip__title">
         {{ t('customHome.exploreCategories') }}
       </h2>
       <HomePinButton v-if="showAddButton" :label="t('customHome.addCategory')"
         @click="emit('add-categories', categories)" />
       <!-- Scroll controls appear only when the single row overflows horizontally. -->
       <div
         v-if="showScrollControls"
         class="home-category-strip__controls"
         :aria-label="t('catalog.horizontalNavigation')"
       >
         <button
           class="home-category-strip__control"
           type="button"
           :disabled="!canScrollLeft"
           :aria-label="t('catalog.scrollLeft')"
           @click="scrollRow('left')"
         >
           <v-icon icon="$NavigateBefore" size="24" aria-hidden="true" />
         </button>
         <button
           class="home-category-strip__control"
           type="button"
           :disabled="!canScrollRight"
           :aria-label="t('catalog.scrollRight')"
           @click="scrollRow('right')"
         >
           <v-icon icon="$NavigateNext" size="24" aria-hidden="true" />
         </button>
       </div>
     </header>

     <div
       ref="viewport"
       class="home-category-strip__viewport"
       :class="{ 'home-category-strip__viewport--single-row': mode === 'single-row' }"
       @scroll="updateScrollState"
     >
       <div
         class="home-category-strip__list"
         :class="{ 'home-category-strip__list--single-row': mode === 'single-row' }"
         role="list"
       >
        <div
         v-for="category in categories"
         :key="category.id"
         class="home-category-strip__item"
         role="listitem"
        >
        <a
          class="home-category-strip__tag"
         :href="categoryHref(category)"
         @click="handleCategorySelect(category, $event)"
       >
         {{ category.label }}
       </a>
        </div>
       </div>
     </div>

   </section>
 </template>

<style scoped>
.home-category-strip {
   display: grid;
   gap: 10px;
   min-width: 0;
 }

.home-category-strip__header {
   display: flex;
   align-items: center;
   gap: 12px;
 }

.home-category-strip__controls {
   display: flex;
   gap: 4px;
   margin-left: auto;
 }

.home-category-strip__control {
   display: grid;
   width: 32px;
   height: 32px;
   place-items: center;
   border: 1px solid var(--border-color-primary);
   border-radius: 50%;
   background: var(--bg-surface);
   color: var(--text-primary);
   cursor: pointer;
 }

.home-category-strip__control:disabled {
   opacity: 0.4;
   cursor: default;
 }

.home-category-strip__control:hover:not(:disabled),
.home-category-strip__control:focus-visible {
   border-color: var(--color-primary);
   background: var(--bg-surface-strong);
 }

.home-category-strip__control:focus-visible {
   outline: var(--common-focus-ring);
   outline-offset: 2px;
 }

.home-category-strip__title {
   margin: 0;
   color: var(--text-primary);
   font-size: clamp(1.05rem, 0.98rem + 0.34vw, 1.35rem);
   font-weight: 700;
   letter-spacing: 0.01em;
 }

.home-category-strip__list {
   display: flex;
   flex-wrap: wrap;
   gap: 8px;
 }

.home-category-strip__viewport {
   min-width: 0;
 }

.home-category-strip__viewport--single-row {
   overflow-x: auto;
   overflow-y: hidden;
   scrollbar-width: none;
 }

.home-category-strip__viewport--single-row::-webkit-scrollbar {
   display: none;
 }

.home-category-strip__list--single-row {
   flex-wrap: nowrap;
 }

.home-category-strip__item {
   display: inline-flex;
   flex: 0 0 auto;
   min-width: 0;
   max-width: 100%;
 }

.home-category-strip__tag {
   display: inline-flex;
   align-items: center;
   max-width: 100%;
   min-height: 26px;
   padding: 10px 20px;
   border: 1px solid var(--border-color-primary);
   border-radius: 999px;
   overflow: hidden;
   background: var(--bg-surface);
   color: var(--text-primary);
   font-size: 1rem;
   font-weight: 700;
   line-height: 1.25;
   text-decoration: none;
   text-overflow: ellipsis;
   white-space: nowrap;
   box-shadow: var(--inset-light);
   transition:
     color var(--duration-fast) ease,
     border-color var(--duration-fast) ease,
     background-color var(--duration-fast) ease;
 }

.home-category-strip__tag:hover {
   border-color: var(--color-primary);
   background: var(--bg-accent-primary);
   color: var(--text-primary);
 }

.home-category-strip__tag:focus-visible {
   outline: var(--common-focus-ring);
   outline-offset: 2px;
 }

@media (max-width: 640px) {
   .home-category-strip__list {
     gap: 6px;
   }

   .home-category-strip__tag {
     min-height: 24px;
     padding: 2px 10px;
   }
}

</style>
