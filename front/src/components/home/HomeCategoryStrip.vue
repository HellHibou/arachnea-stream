<script setup lang="ts">
import { useRouter } from 'vue-router'

import type { HomeCategory } from '@/types/home'

/**
 * Props accepted by the clickable category strip.
 */
interface Props {
   /**
    * Categories exposed by the backend catalog payload.
    */
   categories: HomeCategory[]
}

/** Component props without defaults. */
defineProps<Props>()

const emit = defineEmits<{
  /** Emitted when a category is selected. */
   'select-category': [category: HomeCategory]
}>()

/** Vue Router instance used to resolve category links. */
const router = useRouter()

/**
 * Resolves the internal category route targeted by a category card.
 *
 * @param category Category from the strip.
 * @returns Browser href for the category route.
 */
function categoryHref(category: HomeCategory): string {
  return router.resolve({
    name: 'category',
    params: { categoryKey: category.mergeKey },
  }).href
}

/**
 * Emits the selected category so the parent can load its aggregated catalog.
 *
 * @param category Category selected from the strip.
 */
function handleCategorySelect(category: HomeCategory) {
   emit('select-category', category)
}
</script>

<template>
   <section class="home-category-strip" aria-labelledby="home-category-strip-title">
     <header class="home-category-strip__header">
       <h2 id="home-category-strip-title" class="home-category-strip__title">
         Explorer par categorie
       </h2>
     </header>

     <div class="home-category-strip__list" role="list">
       <a
         v-for="category in categories"
         :key="category.id"
         class="home-category-strip__card"
         role="listitem"
         :href="categoryHref(category)"
         @click.prevent="handleCategorySelect(category)"
       >
         <span
           class="home-category-strip__media"
           :class="{ 'home-category-strip__media--empty': !category.imageUrl }"
           aria-hidden="true"
         >
           <img
             v-if="category.imageUrl"
             class="home-category-strip__image home-category-strip__image--backdrop"
             :src="category.imageUrl"
             loading="lazy"
             alt=""
           >
           <img
             v-if="category.imageUrl"
             class="home-category-strip__image home-category-strip__image--foreground"
             :src="category.imageUrl"
             loading="lazy"
             alt=""
           >
         </span>

         <span class="home-category-strip__content">
           <span class="home-category-strip__label">{{ category.label }}</span>
         </span>
       </a>
     </div>

   </section>
 </template>

<style scoped>
.home-category-strip {
   display: grid;
   gap: 14px;
 }

.home-category-strip__header {
   display: flex;
   align-items: center;
   justify-content: space-between;
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
   display: grid;
   grid-template-columns: repeat(auto-fit, minmax(168px, 1fr));
   gap: 14px;
 }

.home-category-strip__card {
   position: relative;
   display: grid;
   min-height: 128px;
   padding: 0;
   border: 1px solid var(--border-color-primary);
   border-radius: var(--radius);
   overflow: hidden;
   background:
     linear-gradient(180deg, rgba(255, 255, 255, 0.04), rgba(255, 255, 255, 0.02)),
     var(--bg-surface);
   color: var(--text-primary);
   text-align: left;
   cursor: pointer;
   text-decoration: none;
   box-shadow:
     var(--shadow-card),
     var(--inset-light);
   transition:
     transform var(--duration-fast) ease,
     border-color var(--duration-fast) ease,
     box-shadow var(--duration-fast) ease;
 }

.home-category-strip__card:hover {
   transform: translateY(-2px);
   border-color: var(--color-primary);
 }

.home-category-strip__card:focus-visible {
   outline: var(--common-focus-ring);
   outline-offset: 2px;
 }

.home-category-strip__media {
   position: absolute;
   inset: 0;
   isolation: isolate;
 }

.home-category-strip__media::after {
   content: '';
   position: absolute;
   inset: 0;
   z-index: 1;
   pointer-events: none;
 }

.home-category-strip__media--empty {
   background:
     radial-gradient(circle at top left, rgba(70, 163, 255, 0.22), transparent 56%),
     linear-gradient(160deg, rgba(14, 19, 27, 0.94), rgba(24, 33, 47, 0.88));
 }

.home-category-strip__media--empty::after {
   background:
     linear-gradient(180deg, rgba(7, 10, 16, 0.06), rgba(7, 10, 16, 0.88)),
     linear-gradient(135deg, rgba(60, 128, 197, 0.3), rgba(7, 10, 16, 0.12));
 }

.home-category-strip__image {
   position: absolute;
   inset: 0;
   width: 100%;
   height: 100%;
   display: block;
   object-position: center;
 }

.home-category-strip__image--backdrop {
   z-index: 0;
   object-fit: cover;
   filter: var(--filter-media-card-backdrop);
   transform: scale(1.06);
 }

.home-category-strip__image--foreground {
   z-index: 2;
   object-fit: contain;
 }

.home-category-strip__content {
   position: relative;
   z-index: 3;
   display: grid;
   align-content: end;
   gap: 8px;
   min-height: 128px;
   padding: 18px;
 }

.home-category-strip__label {
   color: var(--text-primary);
   font-size: 1.02rem;
   font-weight: 700;
   letter-spacing: 0.01em;
   text-shadow: var(--text-shadow-primary);
 }

@media (max-width: 640px) {
   .home-category-strip__list {
     grid-template-columns: repeat(2, minmax(0, 1fr));
   }

   .home-category-strip__card,
   .home-category-strip__content {
     min-height: 118px;
   }

   .home-category-strip__content {
     padding: 16px 14px;
   }
}

</style>
