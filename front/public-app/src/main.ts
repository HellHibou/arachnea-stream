import './assets/main.css'
import '@mdi/font/css/materialdesignicons.css'
import 'vuetify/styles'

import { createApp } from 'vue'
import AccountSessionGate from './components/accounts/AccountSessionGate.vue'
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
import { initializeI18n } from './i18n'
import { applyTheme, findThemePreset, defaultThemeKey, loadThemes } from '@/services/theme'


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
const app = createApp(desktopShell ? DesktopTabShell : AccountSessionGate)

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
 * Initializes public translation/theme resources and mounts the application root.
 * AccountSessionGate initializes content only after authentication.
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

   await loadThemes()
   applyTheme(findThemePreset(defaultThemeKey))

   app.mount('#app')
 }

void bootstrap()
