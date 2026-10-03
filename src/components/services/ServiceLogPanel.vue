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
  <div class="glass-divider rounded-b-2xl border-t bg-white/30 px-4 py-3 dark:bg-black/20">
    <p class="mb-2 text-xs font-medium tracking-wide text-neutral-400 uppercase">
      Log — {{ serviceId }}
    </p>
    <div
      class="glass-inset rounded-xl p-3 font-mono text-xs text-neutral-700 dark:text-neutral-300"
    >
      <p v-if="lines.length === 0" class="text-neutral-400">
        No log output yet — start the service to see it here.
      </p>
      <p v-for="(line, i) in lines" :key="i">{{ line }}</p>
    </div>
  </div>
</template>
