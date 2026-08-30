<script setup lang="ts">
defineProps<{ placeholder?: string }>()
const model = defineModel<string>({ default: '' })
</script>

<template>
  <div
    class="flex items-center gap-2 rounded-xl border border-neutral-200 bg-white/70 px-3.5 py-2.5 dark:border-neutral-800 dark:bg-neutral-900/60"
  >
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      class="h-4 w-4 shrink-0 text-neutral-400"
    >
      <circle cx="11" cy="11" r="7" />
      <path stroke-linecap="round" d="m20 20-3.5-3.5" />
    </svg>
    <input
      v-model="model"
      type="text"
      :placeholder="placeholder"
      class="w-full bg-transparent text-sm text-neutral-900 outline-none placeholder:text-neutral-400 dark:text-neutral-100"
    />
  </div>
</template>
