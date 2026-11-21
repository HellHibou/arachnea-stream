<script setup lang="ts">
import { useRouter } from 'vue-router'

import { categoryMediaTarget, mediaRoute } from '@/services/mediaRouting'
import type { HomeCategory } from '@/types/home'
import { useI18n } from '@/i18n'
import HomePinButton from '@/components/home/HomePinButton.vue'

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
withDefaults(defineProps<Props>(), { showAddButton: false })
const { t } = useI18n()

const emit = defineEmits<{
  /** Emitted when a category is selected. */
   'select-category': [category: HomeCategory]
   'add-categories': [categories: HomeCategory[]]
}>()

/** Vue Router instance used to resolve category links. */
const router = useRouter()

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
       <h2 id="home-category-strip-title" class="home-category-strip__title">
         {{ t('customHome.exploreCategories') }}
       </h2>
       <HomePinButton v-if="showAddButton" :label="t('customHome.addCategory')"
         @click="emit('add-categories', categories)" />
     </header>

     <div class="home-category-strip__list" role="list">
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

   </section>
 </template>

<style scoped>
.home-category-strip {
   display: grid;
   gap: 10px;
 }

.home-category-strip__header {
   display: flex;
   align-items: center;
   gap: 12px;
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
   padding: 3px 11px;
   border: 1px solid var(--border-color-primary);
   border-radius: 999px;
   overflow: hidden;
   background: var(--bg-surface);
   color: var(--text-primary);
   font-size: 0.8rem;
   font-weight: 600;
   line-height: 1.25;
   text-decoration: none;
   text-overflow: ellipsis;
   white-space: nowrap;
   text-shadow:
     -1px -1px 0 #000,
     0 -1px 0 #000,
     1px -1px 0 #000,
     -1px 0 0 #000,
     1px 0 0 #000,
     -1px 1px 0 #000,
     0 1px 0 #000,
     1px 1px 0 #000;
   box-shadow: var(--inset-light);
   transition:
     color var(--duration-fast) ease,
     border-color var(--duration-fast) ease,
     background-color var(--duration-fast) ease;
 }

.home-category-strip__tag:hover {
   border-color: var(--color-primary);
   background: var(--bg-surface-strong);
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
     font-size: 0.76rem;
   }
}

</style>
