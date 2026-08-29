<script setup lang="ts">
import { usePhpStore } from '@/stores/php'

const store = usePhpStore()
</script>

<template>
  <section>
    <h1 class="text-[28px] leading-tight font-bold tracking-tight">Switch</h1>
    <p class="mt-1 text-sm text-neutral-500">Pick the PHP version your projects run on.</p>

    <div class="mt-5 flex flex-col gap-2.5">
      <button
        v-for="version in store.versions"
        :key="version.id"
        type="button"
        class="flex items-center gap-3 rounded-2xl border p-3.5 text-left transition"
        :class="
          version.active
            ? 'border-red-200 bg-red-50 dark:border-red-500/30 dark:bg-red-500/10'
            : 'border-neutral-200/80 bg-neutral-100/60 hover:border-neutral-300 dark:border-neutral-800 dark:bg-neutral-900/60'
        "
        @click="store.setActive(version.id)"
      >
        <span
          class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-xs font-bold"
          :class="
            version.active
              ? 'bg-red-600 text-white'
              : 'bg-neutral-200/70 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
          "
        >
          PHP
        </span>

        <span class="flex-1 font-mono text-sm font-semibold">{{ version.version }}</span>

        <span
          v-if="version.active"
          class="rounded-full bg-white/80 px-2.5 py-0.5 text-xs font-semibold text-red-600 dark:bg-neutral-900/60 dark:text-red-400"
        >
          Active
        </span>
      </button>
    </div>
  </section>
</template>
