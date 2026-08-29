import { fileURLToPath, URL } from 'node:url'

import { defineConfig, type PluginOption } from 'vite'
import vue from '@vitejs/plugin-vue'
import vuetify from 'vite-plugin-vuetify'

// Marker used by the backend to substitute the runtime `<base href>` in release.
// In dev there is no backend substitution, so neutralize it against the dev root.
const devBaseNeutralizer: PluginOption = {
  name: 'dev-base-neutralizer',
  apply: 'serve',
  transformIndexHtml(html) {
    return html.replace('href="{base}"', 'href="/"')
  },
}

// https://vite.dev/config/
export default defineConfig({
  base: '/admin/',
  plugins: [
    devBaseNeutralizer,
    vue(),
    vuetify({ autoImport: true }),
  ],
  build: {
    outDir: '../dist/admin',
  },
  server: {
    port: 5174,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: true,
      },
    },
  },
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url))
    },
  },
})
