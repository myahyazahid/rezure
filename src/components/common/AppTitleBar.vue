<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { useAppearanceStore } from '@/stores/appearance'
import { useWindowControls } from '@/composables/useWindowControls'

const appearance = useAppearanceStore()
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
  <header data-tauri-drag-region class="flex h-12 shrink-0 items-center gap-3 px-4">
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
        class="glass-inset rounded-md px-1.5 py-0.5 font-mono text-[11px] text-neutral-500 dark:text-neutral-400"
      >
        v{{ version }}
      </span>
    </div>

    <button
      type="button"
      class="glass-btn flex shrink-0 items-center gap-1.5 rounded-full py-1 pr-3 pl-1 text-xs font-medium transition"
      @click="appearance.toggleDark()"
    >
      <span
        class="flex h-5 w-5 items-center justify-center rounded-full"
        :class="
          appearance.isDark ? 'bg-white/10 text-neutral-200' : 'bg-accent-500/15 text-accent-600'
        "
      >
        <svg v-if="appearance.isDark" viewBox="0 0 24 24" fill="currentColor" class="h-3 w-3">
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
      {{ appearance.isDark ? 'Dark' : 'Light' }}
    </button>

    <div class="flex shrink-0 items-center gap-1">
      <button
        type="button"
        title="Minimize"
        class="flex h-7 w-7 items-center justify-center rounded-md text-neutral-500 transition hover:bg-white/60 dark:text-neutral-400 dark:hover:bg-white/10"
        @click="minimize"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-3 w-3">
          <path stroke-linecap="round" d="M5 12h14" />
        </svg>
      </button>
      <button
        type="button"
        title="Maximize"
        class="flex h-7 w-7 items-center justify-center rounded-md text-neutral-500 transition hover:bg-white/60 dark:text-neutral-400 dark:hover:bg-white/10"
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
