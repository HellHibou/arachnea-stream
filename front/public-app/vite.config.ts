/// <reference types="vite/client" />

import { fileURLToPath, URL } from 'node:url'

import { defineConfig, type PluginOption } from 'vite'
import vue from '@vitejs/plugin-vue'
import vuetify from 'vite-plugin-vuetify'

// Base path for the public frontend (served at the site root).
const BASE_URL = '/'

// In dev, replace the {base} marker with the runtime BASE_URL so that relative
// asset/api/locale fetches and the router base resolve correctly even though
// there is no backend performing the substitution.
const devBaseReplacer: PluginOption = {
  name: 'dev-base-replacer',
  apply: 'serve',
  transformIndexHtml(html) {
    return html.replace('href="{base}"', `href="${BASE_URL}"`)
  },
}

// https://vite.dev/config/
export default defineConfig({
  base: BASE_URL,
  plugins: [
    devBaseReplacer,
    vue(),
    vuetify({ autoImport: true }),
  ],
  build: {
    outDir: '../dist',
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
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url))
    },
  },
})