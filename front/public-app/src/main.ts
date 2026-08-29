import './assets/main.css'
import '@mdi/font/css/materialdesignicons.css'
import 'vuetify/styles'

import { createApp } from 'vue'
import App from './App.vue'
import { createVuetify } from 'vuetify'
import { aliases as mdiAliases, mdi } from 'vuetify/iconsets/mdi'
import { createPinia } from 'pinia'
import router from './router'
import { APP_TITLE } from './constants'
import { useServiceMetadata } from './composables/useServiceMetadata'
import { initializeI18n } from './i18n'
import { useStorage } from '@/services/storage'
import { findThemePreset, loadThemes, applyTheme } from '@/services/theme'
import { loadVideoSourceAllowlist } from '@/services/videoSourceAllowlist'


/** The Vuetify instance with configured icon sets and aliases. */
const vuetify = createVuetify({
  icons: {
    defaultSet: 'mdi',
    aliases: {
      ...mdiAliases,
      NavigateBefore: mdiAliases.prev,
      NavigateNext: mdiAliases.next,
      Search: mdiAliases.search,
    },
    sets: {
      mdi,
    },
  },
})

/** The Vue application instance. */
const app = createApp(App)

app
  .use(vuetify)
  .use(createPinia())
  .use(router)

/**
 * Bootstraps the Vue application by initializing i18n and service metadata,
 * then mounts the app to the DOM.
 *
 * @returns Promise that resolves when the application is fully bootstrapped.
 */
async function bootstrap(): Promise<void> {
   document.title = APP_TITLE
   await initializeI18n()
   void useServiceMetadata().load()

   await loadThemes()
   await loadVideoSourceAllowlist()

   const storage = useStorage()
   await storage.initStorage()
   applyTheme(findThemePreset(storage.getParameters().theme.value))

   app.mount('#app')
 }

void bootstrap()
