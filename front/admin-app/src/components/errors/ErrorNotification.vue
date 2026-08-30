<script setup lang="ts">
import { computed, onBeforeUnmount, shallowRef } from 'vue'

import type { BackendCommunicationError } from '@/composables/useErrorNotifications'
import { useI18n } from '@/i18n'

/** Props rendered by one backend communication error notification. */
interface Props {
  /** Structured error displayed by the notification. */
  error: BackendCommunicationError
  /** Whether this notification is currently active for keyboard dismissal. */
  isActive?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  isActive: false,
})

const emit = defineEmits<{
  /** Emitted when the notification should be removed from the queue. */
  dismiss: [id: string]
  /** Emitted when focus enters this notification. */
  activate: [id: string]
}>()

const { t } = useI18n()

/** Whether the code-copy action recently completed. */
const codeCopied = shallowRef(false)
let copyResetTimer: ReturnType<typeof setTimeout> | null = null

/** Localized summary derived from the error classification. */
const summary = computed(() => {
  if (props.error.code === 'network_error') {
    return t('errors.summaries.network')
  }
  if (props.error.code === 'invalid_response') {
    return t('errors.summaries.invalidResponse')
  }
  if (props.error.status === 401 || props.error.status === 403) {
    return t('errors.summaries.accessDenied')
  }
  return t('errors.summaries.server')
})

/** Removes this notification from the global queue. */
function dismiss(): void {
  emit('dismiss', props.error.id)
}

/** Marks this notification as the target of a subsequent Escape press. */
function activate(): void {
  emit('activate', props.error.id)
}

/** Copies the error code when the Clipboard API is available. */
async function copyCode(): Promise<void> {
  if (!navigator.clipboard) {
    return
  }

  try {
    await navigator.clipboard.writeText(props.error.code)
    codeCopied.value = true
    if (copyResetTimer) {
      clearTimeout(copyResetTimer)
    }
    copyResetTimer = setTimeout(() => {
      codeCopied.value = false
      copyResetTimer = null
    }, 1600)
  } catch {
    // The visible code remains selectable when clipboard access is unavailable.
  }
}

/** Releases the delayed copy-status reset when the notification is removed. */
onBeforeUnmount(() => {
  if (copyResetTimer) {
    clearTimeout(copyResetTimer)
  }
})
</script>

<template>
  <article
    class="error-notification"
    :class="{ 'error-notification--active': isActive }"
    role="alert"
    tabindex="0"
    @focusin="activate"
  >
    <div class="error-notification__header">
      <p class="error-notification__eyebrow">{{ t('errors.notifications.title') }}</p>
      <button
        class="error-notification__close"
        type="button"
        :aria-label="t('errors.notifications.close')"
        @click="dismiss"
      >
        <v-icon icon="mdi-close" aria-hidden="true" />
      </button>
    </div>

    <p class="error-notification__message">{{ summary }}</p>

    <details class="error-notification__details">
      <summary>{{ t('errors.notifications.technicalDetails') }}</summary>
      <dl class="error-notification__diagnostics">
        <div v-if="error.status !== undefined">
          <dt>{{ t('errors.notifications.status') }}</dt>
          <dd>{{ error.status }}</dd>
        </div>
        <div>
          <dt>{{ t('errors.notifications.message') }}</dt>
          <dd>{{ error.message }}</dd>
        </div>
        <div>
          <dt>{{ t('errors.notifications.code') }}</dt>
          <dd>
            <button
              class="error-notification__code"
              type="button"
              :aria-label="t('errors.notifications.copyCode', { code: error.code })"
              @click="copyCode"
            >
              <span class="error-notification__code-value">
                <code>{{ error.code }}</code>
                <span v-if="codeCopied" class="error-notification__code-copied" aria-live="polite">
                  {{ t('errors.notifications.codeCopied') }}
                </span>
              </span>
            </button>
          </dd>
        </div>
      </dl>
    </details>
  </article>
</template>

<style scoped>
.error-notification {
  position: relative;
  width: min(480px, calc(100vw - 32px));
  padding: 14px;
  border: 1px solid rgba(var(--v-theme-error), 0.5);
  border-radius: 8px;
  background: rgb(var(--v-theme-surface));
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
  outline: none;
}

.error-notification--active,
.error-notification:focus-visible {
  border-color: rgb(var(--v-theme-error));
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25), 0 0 0 2px rgba(var(--v-theme-error), 0.35);
}

.error-notification__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.error-notification__eyebrow {
  margin: 0;
  color: rgb(var(--v-theme-error));
  font-size: 0.72rem;
  font-weight: 760;
  letter-spacing: 0.1em;
  text-transform: uppercase;
}

.error-notification__close {
  display: grid;
  width: 30px;
  height: 30px;
  place-items: center;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 50%;
  color: rgb(var(--v-theme-on-surface));
  background: transparent;
  cursor: pointer;
}

.error-notification__close:hover,
.error-notification__close:focus-visible {
  border-color: rgba(var(--v-theme-on-surface), 0.25);
  background: rgba(var(--v-theme-on-surface), 0.08);
  outline: none;
}

.error-notification__message {
  margin: 10px 0 12px;
  color: rgb(var(--v-theme-on-surface));
  font-size: 0.94rem;
  line-height: 1.42;
}

.error-notification__code {
  display: inline-flex;
  max-width: 100%;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  border: 1px solid rgba(var(--v-theme-on-surface), 0.15);
  border-radius: 6px;
  color: rgb(var(--v-theme-on-surface));
  background: rgba(var(--v-theme-on-surface), 0.06);
  cursor: pointer;
  text-align: left;
}

.error-notification__code:hover,
.error-notification__code:focus-visible {
  border-color: rgba(var(--v-theme-error), 0.75);
  outline: none;
}

.error-notification__code-value {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 6px;
}

.error-notification__code code {
  overflow: hidden;
  color: rgb(var(--v-theme-on-surface));
  font-size: 0.74rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.error-notification__code-copied {
  color: rgb(var(--v-theme-success));
  font-size: 0.7rem;
  font-weight: 700;
}

.error-notification__details {
  margin-top: 12px;
  color: rgba(var(--v-theme-on-surface), 0.7);
  font-size: 0.82rem;
}

.error-notification__details summary {
  cursor: pointer;
}

.error-notification__diagnostics {
  display: grid;
  gap: 8px;
  margin: 10px 0 0;
}

.error-notification__diagnostics div {
  display: grid;
  gap: 2px;
}

.error-notification__diagnostics dt {
  color: rgb(var(--v-theme-error));
  font-size: 0.7rem;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}

.error-notification__diagnostics dd {
  margin: 0;
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}

@media (max-width: 640px) {
  .error-notification {
    width: min(100%, calc(100vw - 20px));
  }
}
</style>
