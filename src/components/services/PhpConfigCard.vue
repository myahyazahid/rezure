<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { usePhpStore } from '@/stores/php'

const store = usePhpStore()

/** The PATH link is what decides whether the user's *own* terminal reads
 *  this folder too — Rezure's own processes always do. */
const everywhere = computed(() => store.pathStatus?.onPath === true)

onMounted(() => {
  if (!store.configDir) store.fetchConfigDir()
})
</script>

<template>
  <div
    v-if="store.configDir"
    class="rounded-2xl border border-neutral-200/80 bg-neutral-100/60 p-4 dark:border-neutral-800 dark:bg-neutral-900/60"
  >
    <div class="flex items-start gap-3">
      <span
        class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-neutral-200/70 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M4 6h16M4 12h16M4 18h9M17 17l2 2 3-4"
          />
        </svg>
      </span>

      <div class="min-w-0 flex-1">
        <p class="font-semibold text-neutral-900 dark:text-neutral-100">Your PHP settings</p>
        <!-- The one thing worth saying up front: the generated php.ini is
             not a place to write, so nobody should discover that by losing
             an edit to it. -->
        <p class="mt-0.5 text-sm text-neutral-500">
          Rezure rewrites its own <code class="font-mono">php.ini</code> every time PHP starts. Put
          your own settings here instead — any <code class="font-mono">.ini</code> file in this
          folder is loaded after it and wins.
        </p>
      </div>

      <button
        type="button"
        class="shrink-0 rounded-full border border-neutral-200 bg-white px-4 py-2 text-sm font-semibold text-neutral-700 transition hover:bg-neutral-100 dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-200"
        @click="store.openConfigDir"
      >
        Open folder
      </button>
    </div>

    <div class="mt-3 flex flex-col gap-1 text-xs">
      <p class="text-neutral-500">
        <template v-if="everywhere">
          Read by your sites, by Composer, and by <code class="font-mono">php</code> in your own
          terminals.
        </template>
        <template v-else>
          Read by your sites and by Composer. Turn on
          <strong>Use Rezure's PHP everywhere</strong> above for <code class="font-mono">php</code>
          in your own terminals to read it too.
        </template>
      </p>
      <p class="text-neutral-500">
        Restart PHP for a change to reach running sites. Files load in alphabetical order, so
        <code class="font-mono">90-</code> wins over <code class="font-mono">10-</code>.
      </p>
      <button
        type="button"
        class="truncate text-left font-mono text-neutral-400 underline"
        :title="store.configDir"
        @click="store.openConfigDir"
      >
        {{ store.configDir }}
      </button>
    </div>
  </div>
</template>
