import { fileURLToPath, URL } from 'node:url'

import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import vueDevTools from 'vite-plugin-vue-devtools'
import tailwindcss from '@tailwindcss/vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [vue(), vueDevTools(), tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  // Tauri expects a fixed port, fails if that port is not available
  server: {
    strictPort: true,
  },
  // Env variables starting with the item of `envPrefix` will be exposed in tauri's source code through `import.meta.env`.
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    // Rezure targets Windows only — WebView2 is Chromium-based.
    target: 'chrome105',
    minify: !process.env.TAURI_ENV_DEBUG ? ('esbuild' as const) : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
})
