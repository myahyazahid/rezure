<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { useAppearanceStore } from '@/stores/appearance'
import { useWindowControls } from '@/composables/useWindowControls'
// The R cropped from assets/logo-master.png.
import logoR from '@/assets/logo-r.png'

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
  <header data-tauri-drag-region class="flex h-12 shrink-0 items-center gap-3 pr-4">
    <!-- Same ml-3 w-60 as the sidebar below, so the brand sits centred over it;
         nudged down to sit closer to the sidebar than to the window edge.
         Baseline-aligned: the pieces mix sizes and a serif, so centring them
         leaves "Redscale" riding high. -->
    <div
      data-tauri-drag-region
      class="ml-3 flex w-60 shrink-0 translate-y-1.5 items-baseline justify-center gap-2 overflow-hidden"
    >
      <!-- Wordmark: the R from the logo, then the rest set in type in the
           logo's own red. Baseline-aligned, so the R sits on the baseline. -->
      <span class="flex items-baseline text-[#ea0619]">
        <span class="sr-only">Rezure</span>
        <img :src="logoR" alt="" aria-hidden="true" class="h-4.5 w-auto shrink-0" />
        <span aria-hidden="true" class="ml-px text-[19px] leading-none font-semibold tracking-tight"
          >ezure</span
        >
      </span>
      <span class="text-sm text-neutral-500">by</span>
      <!-- Larger than "by": Cormorant's small x-height reads a size down. -->
      <span class="brand-wordmark text-[17px] leading-none text-red-600 dark:text-red-500"
        >Redscale</span
      >
      <span
        v-if="version"
        class="glass-inset rounded-md px-1.5 py-0.5 font-mono text-[11px] text-neutral-500 dark:text-neutral-400"
      >
        v{{ version }}
      </span>
    </div>

    <div data-tauri-drag-region class="flex-1 self-stretch"></div>

    <button
      type="button"
      class="glass-btn flex shrink-0 items-center gap-1.5 rounded-full py-1 pr-3 pl-1 text-xs font-medium transition disabled:cursor-default disabled:opacity-60"
      :disabled="appearance.isDarkForced"
      :title="
        appearance.isDarkForced ? 'This theme always uses light text on a dark backdrop' : undefined
      "
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
