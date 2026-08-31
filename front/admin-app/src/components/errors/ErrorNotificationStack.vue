<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, shallowRef, watch } from 'vue'

import type { BackendCommunicationError } from '@/composables/useErrorNotifications'
import { useI18n } from '@/i18n'
import ErrorNotification from './ErrorNotification.vue'

/** Maximum number of non-modal notifications simultaneously displayed. */
const MAX_VISIBLE_NOTIFICATIONS = 5

/** Props accepted by the global error-notification stack. */
interface Props {
  /** Ordered notification queue supplied by the global composable. */
  notifications: readonly BackendCommunicationError[]
}

const props = defineProps<Props>()

const emit = defineEmits<{
  /** Emitted when a notification is dismissed by code. */
  dismiss: [id: string]
}>()

const { t } = useI18n()

/** Identifier for the notification that Escape should dismiss. */
const activeId = shallowRef<string | null>(null)

/** First five queued notifications, kept in their arrival order. */
const visibleNotifications = computed(() => props.notifications.slice(0, MAX_VISIBLE_NOTIFICATIONS))

/** Dismisses the active notification, falling back to the first visible item. */
function dismissActiveNotification(): void {
  const active = visibleNotifications.value.find((notification) => notification.id === activeId.value)
    ?? visibleNotifications.value[0]
  if (active) {
    emit('dismiss', active.id)
  }
}

/** Handles Escape globally without creating a modal keyboard trap. */
function handleWindowKeyDown(event: KeyboardEvent): void {
  if (event.key !== 'Escape' || visibleNotifications.value.length === 0) {
    return
  }

  event.preventDefault()
  dismissActiveNotification()
}

/** Keeps the active identifier valid as queued notifications are dismissed. */
watch(visibleNotifications, (notifications) => {
  if (!notifications.some((notification) => notification.id === activeId.value)) {
    activeId.value = notifications[0]?.id ?? null
  }
}, { immediate: true })

/** Installs the Escape listener while the stack is mounted. */
onMounted(() => {
  window.addEventListener('keydown', handleWindowKeyDown)
})

/** Removes the Escape listener when the app shell unmounts. */
onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleWindowKeyDown)
})
</script>

<template>
  <Teleport to="body">
    <section class="error-notification-stack" :aria-label="t('errors.notifications.title')">
      <TransitionGroup name="error-notification-stack__item">
        <ErrorNotification
          v-for="notification in visibleNotifications"
          :key="notification.id"
          :error="notification"
          :is-active="notification.id === activeId"
          @activate="activeId = $event"
          @dismiss="emit('dismiss', $event)"
        />
      </TransitionGroup>
    </section>
  </Teleport>
</template>

<style scoped>
.error-notification-stack {
  position: fixed;
  right: 16px;
  bottom: 16px;
  z-index: 2500;
  display: grid;
  gap: 10px;
  pointer-events: none;
}

.error-notification-stack :deep(.error-notification) {
  pointer-events: auto;
}

.error-notification-stack__item-enter-active,
.error-notification-stack__item-leave-active {
  transition: opacity 180ms ease, transform 180ms ease;
}

.error-notification-stack__item-enter-from,
.error-notification-stack__item-leave-to {
  opacity: 0;
  transform: translateX(18px);
}

@media (prefers-reduced-motion: reduce) {
  .error-notification-stack__item-enter-active,
  .error-notification-stack__item-leave-active {
    transition: none;
  }
}

@media (max-width: 640px) {
  .error-notification-stack {
    right: 10px;
    bottom: 10px;
    left: 10px;
  }
}
</style>
