import './assets/main.css'
import '@mdi/font/css/materialdesignicons.css'
import 'vuetify/styles'

import { createApp } from 'vue'
import App from './App.vue'
import { createVuetify } from 'vuetify'
import { aliases as mdiAliases, mdi } from 'vuetify/iconsets/mdi'
import { createPinia } from 'pinia'
import router from './router'
import { useServiceMetadata } from './composables/useServiceMetadata'
import { initializeI18n } from './i18n'


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

const app = createApp(App)

app
  .use(vuetify)
  .use(createPinia())
  .use(router)

async function bootstrap(): Promise<void> {
  await initializeI18n()
  void useServiceMetadata().load()

  app.mount('#app')
}

void bootstrap()
