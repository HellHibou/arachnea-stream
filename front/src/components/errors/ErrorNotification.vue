<script setup lang="ts">
import { computed, onBeforeUnmount, shallowRef } from 'vue'

import { t } from '@/i18n'
import type { ScraperExecutionError } from '@/types/scraperError'

/** Props rendered by one source-scoped execution error notification. */
interface Props {
  /** Structured error displayed by the notification. */
  error: ScraperExecutionError
  /** Whether this notification is currently active for keyboard dismissal. */
  isActive?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  isActive: false,
})

const emit = defineEmits<{
  /** Emitted when the notification should be removed from the queue. */
  dismiss: [code: string]
  /** Emitted when focus enters this notification. */
  activate: [code: string]
}>()

/** Whether the code-copy action recently completed. */
const codeCopied = shallowRef(false)
let copyResetTimer: ReturnType<typeof setTimeout> | null = null

/** Localized summary that preserves technical detail for the collapsed disclosure. */
const summary = computed(() => props.error.source
  ? t('errors.notifications.messageWithSource', {
      operation: props.error.operation,
      source: props.error.source,
    })
  : t('errors.notifications.messageWithoutSource', {
      operation: props.error.operation,
    }))

/** Removes this notification from the global queue. */
function dismiss(): void {
  emit('dismiss', props.error.code)
}

/** Marks this notification as the target of a subsequent Escape press. */
function activate(): void {
  emit('activate', props.error.code)
}

/** Copies the correlation code when the Clipboard API is available. */
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

    <button
      class="error-notification__code"
      type="button"
      :aria-label="t('errors.notifications.copyCode', { code: error.code })"
      @click="copyCode"
    >
      <span>{{ t('errors.notifications.code') }}</span>
      <span class="error-notification__code-value">
        <code>{{ error.code }}</code>
        <span v-if="codeCopied" class="error-notification__code-copied" aria-live="polite">
          {{ t('errors.notifications.codeCopied') }}
        </span>
      </span>
    </button>

    <details class="error-notification__details">
      <summary>{{ t('errors.notifications.technicalDetails') }}</summary>
      <dl class="error-notification__diagnostics">
        <div>
          <dt>{{ t('errors.notifications.origin') }}</dt>
          <dd>{{ error.origin }}</dd>
        </div>
        <div>
          <dt>{{ t('errors.notifications.message') }}</dt>
          <dd>{{ error.message }}</dd>
        </div>
      </dl>
    </details>
  </article>
</template>

<style scoped>
.error-notification {
  position: relative;
  width: min(390px, calc(100vw - 32px));
  padding: 14px;
  border: 1px solid color-mix(in srgb, #ff8e72 52%, var(--border-color-primary));
  border-radius: var(--radius);
  background: color-mix(in srgb, #28191f 88%, var(--bg-transparent));
  backdrop-filter: var(--backdrop-filter-strong);
  -webkit-backdrop-filter: var(--backdrop-filter-strong);
  box-shadow: var(--shadow-heavy), var(--inset-light);
  outline: none;
}

.error-notification--active,
.error-notification:focus-visible {
  border-color: #ffb199;
  box-shadow: var(--shadow-heavy), var(--inset-light), 0 0 0 2px rgb(255 177 153 / 36%);
}

.error-notification__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.error-notification__eyebrow {
  margin: 0;
  color: #ffb199;
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
  color: var(--text-color-primary);
  background: transparent;
  cursor: pointer;
}

.error-notification__close:hover,
.error-notification__close:focus-visible {
  border-color: var(--border-color-primary);
  background: rgb(255 255 255 / 10%);
  outline: none;
}

.error-notification__message {
  margin: 10px 0 12px;
  color: var(--text-color-primary);
  font-size: 0.94rem;
  line-height: 1.42;
}

.error-notification__code {
  display: flex;
  width: 100%;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 10px;
  border: 1px solid rgb(255 255 255 / 13%);
  border-radius: calc(var(--radius) * 0.7);
  color: var(--text-color-primary);
  background: rgb(0 0 0 / 20%);
  cursor: pointer;
  text-align: left;
}

.error-notification__code:hover,
.error-notification__code:focus-visible {
  border-color: rgb(255 177 153 / 75%);
  outline: none;
}

.error-notification__code span {
  color: var(--text-color-secondary);
  font-size: 0.72rem;
  text-transform: uppercase;
}

.error-notification__code-value {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 6px;
}

.error-notification__code code {
  overflow: hidden;
  color: #ffd3c6;
  font-size: 0.74rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.error-notification__code-copied {
  color: #c9f5d1;
  font-size: 0.7rem;
  font-weight: 700;
}

.error-notification__details {
  margin-top: 12px;
  color: var(--text-color-secondary);
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
  color: #ffb199;
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
