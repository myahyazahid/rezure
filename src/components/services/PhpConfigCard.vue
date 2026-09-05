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
  <div v-if="store.configDir" class="p-4">
    <div class="flex items-start justify-between gap-4">
      <div class="min-w-0">
        <p class="font-semibold text-neutral-900 dark:text-neutral-100">Your PHP settings</p>
        <!-- The one thing worth saying up front: the generated php.ini is
             not a place to write, so nobody should discover that by losing
             an edit to it. -->
        <p class="mt-0.5 text-xs text-neutral-500">
          Rezure rewrites its own <code class="font-mono">php.ini</code> on every start. Put your
          own settings here instead — any <code class="font-mono">.ini</code> file in this folder
          loads after it and wins.
        </p>
      </div>

      <button
        type="button"
        class="shrink-0 rounded-full border border-neutral-200 bg-white px-4 py-1.5 text-xs font-semibold text-neutral-700 transition hover:bg-neutral-100 dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
        @click="store.openConfigDir"
      >
        Open folder
      </button>
    </div>

    <div class="mt-3 flex flex-col gap-1.5">
      <p
        class="truncate rounded-lg bg-neutral-100 px-2.5 py-1.5 font-mono text-xs text-neutral-500 dark:bg-neutral-800/60"
        :title="store.configDir"
      >
        {{ store.configDir }}
      </p>
      <p class="text-xs text-neutral-500">
        Restart PHP for a change to reach running sites. Files load in alphabetical order, so
        <code class="font-mono">90-</code> wins over <code class="font-mono">10-</code>.
      </p>
      <p v-if="!everywhere" class="text-xs text-neutral-500">
        Read by your sites and by Composer. Turn on <strong>Use Rezure's PHP everywhere</strong> for
        <code class="font-mono">php</code> in your own terminals to read it too.
      </p>
    </div>
  </div>
</template>
