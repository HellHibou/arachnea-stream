<script setup lang="ts">
import { onBeforeUnmount, shallowRef, useTemplateRef } from 'vue'

/** Props for the document hosting the decorative background player. */
defineProps<{
  /** Accessible title of the decorative frame. */
  title: string
}>()

/** Same-origin frame used only to isolate the video from page context menus. */
const frame = useTemplateRef<HTMLIFrameElement>('frame')
/** Mount target kept outside the page document while preserving the Vue tree. */
const target = shallowRef<HTMLElement | null>(null)
/** Copies of application styles, including lazily loaded player styles. */
const styleCopies = new Map<Element, Element>()
/** Observer keeping production styles and development style updates in sync. */
let styleObserver: MutationObserver | null = null

/** Copies application styles into the frame without reloading unchanged sheets. */
function syncStyles() {
  const frameDocument = frame.value?.contentDocument
  if (!frameDocument) {
    return
  }

  const sources = new Set(document.head.querySelectorAll('style, link[rel="stylesheet"]'))
  for (const [source, copy] of styleCopies) {
    if (!sources.has(source)) {
      copy.remove()
      styleCopies.delete(source)
    }
  }

  let previous: Element | null = frameDocument.head.querySelector('base')
  for (const source of sources) {
    let copy = styleCopies.get(source)
    if (!copy || !source.isEqualNode(copy)) {
      copy?.remove()
      copy = source.cloneNode(true) as Element
      styleCopies.set(source, copy)
    }
    const next = previous?.nextSibling ?? null
    if (copy !== next) {
      frameDocument.head.insertBefore(copy, next)
    }
    previous = copy
  }
}

/** Prepares a fresh frame document before mounting the existing player in it. */
function handleLoad() {
  const frameDocument = frame.value?.contentDocument
  if (!frameDocument || target.value?.ownerDocument === frameDocument) {
    return
  }

  styleObserver?.disconnect()
  styleCopies.clear()

  const base = frameDocument.createElement('base')
  base.href = document.baseURI
  frameDocument.head.append(base)
  frameDocument.documentElement.style.background = 'transparent'
  frameDocument.body.style.cssText = 'margin: 0; overflow: hidden; background: transparent; pointer-events: none;'

  syncStyles()
  styleObserver = new MutationObserver(syncStyles)
  styleObserver.observe(document.head, {
    childList: true,
    subtree: true,
    characterData: true,
    attributes: true,
  })
  target.value = frameDocument.body
}

onBeforeUnmount(() => {
  styleObserver?.disconnect()
  styleCopies.clear()
})
</script>

<template>
  <!-- Firefox's video overlay detection must not find the decorative video in the page document. -->
  <iframe
    ref="frame"
    class="background-video-frame"
    :title="title"
    srcdoc="<!doctype html><html><head></head><body></body></html>"
    allow="autoplay; encrypted-media; picture-in-picture"
    aria-hidden="true"
    tabindex="-1"
    @load="handleLoad"
  />
  <Teleport v-if="target" :to="target">
    <slot />
  </Teleport>
</template>

<style scoped>
.background-video-frame {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  border: 0;
  pointer-events: none;
}
</style>
