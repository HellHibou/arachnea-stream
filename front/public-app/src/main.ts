import './assets/main.css'
import '@mdi/font/css/materialdesignicons.css'
import 'vuetify/styles'

import { createApp } from 'vue'
import App from './App.vue'
import DesktopTabShell from './components/desktop/DesktopTabShell.vue'
import {
  isDesktopTabPage,
  isDesktopTabShell,
  reportDesktopTabsFullscreen,
} from './services/desktopTabs'
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

/** Whether this webview renders the desktop tab strip. */
const desktopShell = isDesktopTabShell()
/** Whether this webview renders a page managed by the native desktop tab host. */
const desktopTabPage = isDesktopTabPage()
/** The Vue application instance for this webview. */
const app = createApp(desktopShell ? DesktopTabShell : App)

if (desktopTabPage) {
  document.addEventListener('fullscreenchange', () => {
    void reportDesktopTabsFullscreen(document.fullscreenElement !== null)
  })
}

app
  .use(vuetify)
  .use(createPinia())

if (!desktopShell) app.use(router)

/**
 * Initializes translations for every webview and page services for content views,
 * then mounts the appropriate application root.
 *
 * @returns Promise that resolves when the application is fully bootstrapped.
 */
async function bootstrap(): Promise<void> {
   document.title = APP_TITLE
   await initializeI18n()
   if (desktopShell) {
     app.mount('#app')
     return
   }
   void useServiceMetadata().load()

   await loadThemes()
   await loadVideoSourceAllowlist()

   const storage = useStorage()
   await storage.initStorage()
   applyTheme(findThemePreset(storage.getParameters().theme.value))

   app.mount('#app')
 }

void bootstrap()
