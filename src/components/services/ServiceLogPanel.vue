<script setup lang="ts">
import { computed } from 'vue'
import { useLogsStore } from '@/stores/logs'

const props = defineProps<{ serviceId: string }>()

const store = useLogsStore()

/** Oldest-first, most recent 20 lines — natural top-to-bottom reading order. */
const lines = computed(() =>
  store.entries
    .filter((entry) => entry.service === props.serviceId)
    .slice(0, 20)
    .reverse()
    .map((entry) => `[${entry.time}] ${entry.message}`),
)
</script>

<template>
  <div
    class="border-t border-neutral-200 bg-neutral-50 px-4 py-3 dark:border-neutral-800 dark:bg-neutral-950/50"
  >
    <p class="mb-2 text-xs font-medium tracking-wide text-neutral-400 uppercase">
      Log — {{ serviceId }}
    </p>
    <div class="rounded-lg bg-neutral-900 p-3 font-mono text-xs text-neutral-300 dark:bg-black">
      <p v-if="lines.length === 0" class="text-neutral-500">
        No log output yet — start the service to see it here.
      </p>
      <p v-for="(line, i) in lines" :key="i">{{ line }}</p>
    </div>
  </div>
</template>
