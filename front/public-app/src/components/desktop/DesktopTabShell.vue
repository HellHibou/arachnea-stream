<script setup lang="ts">
import { nextTick, watch } from 'vue'
import { useDesktopTabs } from '@/composables/useDesktopTabs'
import { t } from '@/i18n'

const { state, error, act } = useDesktopTabs()

watch(() => state.value.active, async (id) => {
  await nextTick()
  document.getElementById(`strip-${id}`)?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
})

/**
 * Moves keyboard selection between tabs and restores focus to its strip button.
 * @param event - Keyboard event from a tab button.
 * @param index - Position of the current tab.
 */
async function navigateTabs(event: KeyboardEvent, index: number) {
  const count = state.value.tabs.length
  let next = index
  if (event.key === 'ArrowRight') next = (index + 1) % count
  else if (event.key === 'ArrowLeft') next = (index + count - 1) % count
  else if (event.key === 'Home') next = 0
  else if (event.key === 'End') next = count - 1
  else return
  event.preventDefault()
  const tab = state.value.tabs[next]
  if (!tab) return
  await act('activate', tab.id)
  document.getElementById(`strip-${tab.id}`)?.focus()
}
</script>

<template>
  <div v-if="state.tabs.length > 1" class="desktop-strip">
    <div class="desktop-tabs" role="tablist" :aria-label="t('desktopTabs.label')">
      <div v-for="(tab, index) in state.tabs" :key="tab.id" class="desktop-tab" :class="{ active: tab.id === state.active }">
        <button
          :id="`strip-${tab.id}`"
          class="desktop-tab-title"
          role="tab"
          :aria-selected="tab.id === state.active"
          :tabindex="tab.id === state.active ? 0 : -1"
          :title="tab.title"
          @click="act('activate', tab.id)"
          @keydown="navigateTabs($event, index)"
        >{{ tab.title }}</button>
        <button class="desktop-tab-close" :aria-label="t('desktopTabs.close', { title: tab.title })" @click="act('close', tab.id)">×</button>
      </div>
    </div>
    <button class="desktop-tab-add" :aria-label="t('desktopTabs.new')" :title="t('desktopTabs.new')" @click="act('open')">+</button>
    <span v-if="error" class="desktop-tab-error" role="alert" :title="error">{{ t('desktopTabs.error') }}</span>
  </div>
</template>

<style scoped>
.desktop-strip {
  display: flex;
  align-items: center;
  height: 44px;
  padding: 4px 8px;
  gap: 6px;
  color: #e8e8ed;
  background: #15151d;
  box-sizing: border-box;
}

.desktop-tabs {
  display: flex;
  flex: 1;
  gap: 4px;
  min-width: 0;
  overflow-x: auto;
  overflow-y: hidden;
  scrollbar-width: thin;
}

.desktop-tab {
  display: flex;
  flex: 0 1 220px;
  min-width: 100px;
  border-radius: 6px;
  background: #24242e;
}

.desktop-tab.active {
  background: #404052;
}

.desktop-strip button {
  height: 32px;
  border: 0;
  border-radius: 6px;
  color: inherit;
  background: transparent;
  cursor: pointer;
}

.desktop-strip button:hover {
  background: #ffffff18;
}

.desktop-strip button:focus-visible {
  outline: 2px solid #b4a0ff;
  outline-offset: -2px;
}

.desktop-tab-title {
  flex: 1;
  min-width: 0;
  padding: 0 10px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-align: left;
}

.desktop-tab-close,
.desktop-tab-add {
  flex: 0 0 32px;
  font-size: 21px;
}

.desktop-tab-error {
  color: #ffb4b4;
  font-size: 12px;
}
</style>
