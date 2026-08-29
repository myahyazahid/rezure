<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { useTheme } from '@/composables/useTheme'
import { useWindowControls } from '@/composables/useWindowControls'

const { theme, toggle } = useTheme()
const { minimize, toggleMaximize, close } = useWindowControls()

const version = ref('')

onMounted(async () => {
  try {
    version.value = await getVersion()
  } catch {
    // Version is decorative — a failure here should not break the title bar.
  }
})
</script>

<template>
  <header
    data-tauri-drag-region
    class="flex h-12 shrink-0 items-center gap-3 border-b border-neutral-200/70 px-4 dark:border-neutral-800"
  >
    <!-- Decorative window dots, part of the app's visual identity. -->
    <div class="flex shrink-0 items-center gap-2">
      <span class="h-3 w-3 rounded-full bg-[#ff5f57]"></span>
      <span class="h-3 w-3 rounded-full bg-[#febc2e]"></span>
      <span class="h-3 w-3 rounded-full bg-[#28c840]"></span>
    </div>

    <div data-tauri-drag-region class="flex min-w-0 flex-1 items-center gap-2">
      <span class="text-[15px] font-bold tracking-tight">Rezure</span>
      <span class="text-xs text-neutral-500">by</span>
      <span class="text-xs font-semibold text-red-600 dark:text-red-500">Redscale</span>
      <span
        v-if="version"
        class="rounded-md bg-neutral-100 px-1.5 py-0.5 font-mono text-[11px] text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400"
      >
        v{{ version }}
      </span>
    </div>

    <button
      type="button"
      class="flex shrink-0 items-center gap-1.5 rounded-full border border-neutral-200 py-1 pr-3 pl-1 text-xs font-medium transition hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
      @click="toggle"
    >
      <span
        class="flex h-5 w-5 items-center justify-center rounded-full"
        :class="
          theme === 'dark'
            ? 'bg-neutral-700 text-neutral-200'
            : 'bg-red-100 text-red-600 dark:bg-red-500/20'
        "
      >
        <svg v-if="theme === 'dark'" viewBox="0 0 24 24" fill="currentColor" class="h-3 w-3">
          <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79Z" />
        </svg>
        <svg
          v-else
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
          class="h-3 w-3"
        >
          <circle cx="12" cy="12" r="4.5" />
          <path
            stroke-linecap="round"
            d="M12 1.5v2M12 20.5v2M4.2 4.2l1.4 1.4M18.4 18.4l1.4 1.4M1.5 12h2M20.5 12h2M4.2 19.8l1.4-1.4M18.4 5.6l1.4-1.4"
          />
        </svg>
      </span>
      {{ theme === 'dark' ? 'Dark' : 'Light' }}
    </button>

    <div class="flex shrink-0 items-center gap-1">
      <button
        type="button"
        title="Minimize"
        class="flex h-7 w-7 items-center justify-center rounded-md text-neutral-500 transition hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
        @click="minimize"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-3 w-3">
          <path stroke-linecap="round" d="M5 12h14" />
        </svg>
      </button>
      <button
        type="button"
        title="Maximize"
        class="flex h-7 w-7 items-center justify-center rounded-md text-neutral-500 transition hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
        @click="toggleMaximize"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-3 w-3">
          <rect x="4.5" y="4.5" width="15" height="15" rx="2.5" />
        </svg>
      </button>
      <button
        type="button"
        title="Close"
        class="flex h-7 w-7 items-center justify-center rounded-md text-neutral-500 transition hover:bg-red-600 hover:text-white dark:text-neutral-400"
        @click="close"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-3 w-3">
          <path stroke-linecap="round" d="M6 6l12 12M18 6L6 18" />
        </svg>
      </button>
    </div>
  </header>
</template>
