/// <reference types="vite/client" />

import { fileURLToPath, URL } from 'node:url'

import { defineConfig, type PluginOption } from 'vite'
import vue from '@vitejs/plugin-vue'
import vuetify from 'vite-plugin-vuetify'

// Development has a fixed mount point, while production uses relative asset
// URLs so the backend can mount the bundle below any entrypoint root.
const DEV_BASE_URL = '/'

// In dev, replace the {base} marker with the fixed development base so that relative
// asset/api/locale fetches and the router base resolve correctly even though
// there is no backend performing the substitution.
const devBaseReplacer: PluginOption = {
  name: 'dev-base-replacer',
  apply: 'serve',
  transformIndexHtml(html) {
    return html.replace('href="{base}"', `href="${DEV_BASE_URL}"`)
  },
}

// https://vite.dev/config/
export default defineConfig(({ command }) => ({
  base: command === 'serve' ? DEV_BASE_URL : './',
  plugins: [
    devBaseReplacer,
    vue(),
    vuetify({ autoImport: true }),
  ],
  build: {
    outDir: '../dist',
    rollupOptions: {
      output: {
        // Split the player into dedicated chunks: the shared player business
        // logic (composables + media components used by lives, entry details
        // and the background video), the Video.js core, its plugins, and the
        // DASH playback stack (handler and dash.js engine) are cached
        // independently from the main entry and from each other.
        manualChunks(id) {
          // CommonJS interop helpers are shared by the Video.js chunks; keeping
          // them in the core chunk avoids a circular chunk dependency
          // (core imports helpers, plugins import core).
          if (id.includes('commonjsHelpers')) {
            return 'video-player'
          }
          if (id.includes('node_modules')) {
            if (/[\\/]node_modules[\\/]dashjs(?:[\\/][^\\/]+)?/.test(id)) {
              return 'video-player-dashjs'
            }
            if (id.includes('videojs-contrib-dash')) {
              return 'video-player-dash'
            }
            if (/[\\/]node_modules[\\/](video\.js(?:[\\/][^\\/]+)?|videojs-vtt\.js(?:[\\/][^\\/]+)?|@videojs[\\/](?:http-streaming|vhs-utils|xhr)(?:[\\/][^\\/]+)?|global[\\/](?:document|window))/.test(id)) {
              return 'video-player'
            }
            if (/[\\/]node_modules[\\/](videojs-[^\\/]+|@videojs[\\/][^\\/]+)/.test(id)) {
              return 'video-player-plugins'
            }
            return undefined
          }
          if (/[\\/]src[\\/](components[\\/]media|composables[\\/]video)[\\/]/.test(id)) {
            return 'video-player-core'
          }
          return undefined
        },
      },
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: true,
        // Align Origin with the backend host so write-origin checks
        // (Origin vs Host) pass in development.
        headers: { origin: 'http://127.0.0.1:8080' },
      },
    },
  },
  resolve: {
    // videojs-contrib-dash declares an older video.js range, so the bundled
    // dependency is forced onto the application copy to keep one player instance.
    dedupe: ['video.js'],
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url))
    },
  },
}))
