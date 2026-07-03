<script setup lang="ts">
import { useI18n } from '@/i18n'

/**
 * Props accepted by the shared media card action button.
 */
interface Props {
  /**
   * Indicates whether the card can request the detailed entry.
   */
  canSelectItem: boolean
  /**
   * Applies the list layout variant to the action button.
   * @default false
   */
  isList?: boolean
}

/** Component props with applied defaults. */
withDefaults(defineProps<Props>(), {
  isList: false,
})

const emit = defineEmits<{
  /** Emitted when the action button is clicked. */
  select: []
}>()
/** Internationalization utilities. */
const { t } = useI18n()

/**
 * Emits the current selection when the card action is available.
 */
function handleSelect() {
  emit('select')
}
</script>

<template>
  <button
    class="media-card__action"
    :class="[
      { 'media-card__action--disabled': !canSelectItem },
      { 'media-card__action--list': isList },
    ]"
    type="button"
    :disabled="!canSelectItem"
    @click.stop="handleSelect"
  >
    {{ t('entry.watch') }}
  </button>
</template>

<style scoped>
.media-card__action {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  margin-top: auto;
  align-self: center;
  min-height: 40px;
  max-width: 100%;
  padding: 0 16px;
  border: 0;
  border-radius: 999px;
  background: var(--bg-accent-blue);
  box-shadow: 0 10px 18px var(--shadow-color);
  color: var(--text-primary);
  font: inherit;
  font-size: 0.96rem;
  font-weight: 700;
  cursor: pointer;
  transition:
    transform var(--duration-fast) ease,
    filter var(--duration-fast) ease;
}

.media-card__action--list {
  align-self: flex-start;
  margin-top: 0;
}

.media-card__action:hover {
  transform: translateY(-1px);
  filter: brightness(1.08);
}

.media-card__action:focus-visible {
  outline: var(--common-focus-ring);
  outline-offset: 2px;
}

.media-card__action--disabled {
  cursor: default;
  opacity: 0.46;
}

@media (max-width: 720px) {
  .media-card__action--list {
    width: 100%;
    justify-content: center;
  }
}
</style>
