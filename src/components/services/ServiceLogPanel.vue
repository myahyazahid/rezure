<script setup lang="ts">
defineProps<{ serviceId: string }>()

const lines = [
  '[info] service initialized',
  '[info] listening for connections',
  '[info] waiting for requests…',
]
</script>

<template>
  <div
    class="border-t border-neutral-200 bg-neutral-50 px-4 py-3 dark:border-neutral-800 dark:bg-neutral-950/50"
  >
    <p class="mb-2 text-xs font-medium tracking-wide text-neutral-400 uppercase">
      Log — {{ serviceId }}
    </p>
    <div class="rounded-lg bg-neutral-900 p-3 font-mono text-xs text-neutral-300 dark:bg-black">
      <p v-for="(line, i) in lines" :key="i">{{ line }}</p>
    </div>
  </div>
</template>
