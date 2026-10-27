import './assets/main.css'
// Bundled rather than fetched, so the title-bar "Redscale" wordmark renders
// offline. Latin, one weight, italic: that's all the wordmark uses.
import '@fontsource/cormorant-garamond/latin-600-italic.css'

import { createApp } from 'vue'
import { createPinia } from 'pinia'

import App from './App.vue'
import router from './router'
import { useAppearanceStore } from './stores/appearance'
import { useDecorationsStore } from './stores/decorations'

const app = createApp(App)

app.use(createPinia())
app.use(router)

// Created before mount so the cached theme is on <html> for the first paint;
// `load()` then reconciles it with settings.json.
useAppearanceStore().load()
useDecorationsStore().load()

app.mount('#app')
