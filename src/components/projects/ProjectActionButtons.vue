<script setup lang="ts">
defineProps<{ domain: string; path: string }>()

const BUTTON_CLASS =
  'flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-neutral-200 bg-white/60 text-neutral-500 transition hover:bg-white dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-400 dark:hover:bg-neutral-800'
</script>

<template>
  <div class="flex shrink-0 items-center gap-1.5">
    <button :class="BUTTON_CLASS" type="button" :title="`Open https://${domain}`">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
        <circle cx="12" cy="12" r="9" />
        <path
          stroke-linecap="round"
          d="M3 12h18M12 3c2.5 2.7 2.5 15.3 0 18M12 3c-2.5 2.7-2.5 15.3 0 18"
        />
      </svg>
    </button>
    <button :class="BUTTON_CLASS" type="button" :title="`Open ${path}`">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M3 7a2 2 0 0 1 2-2h3.6l2 2.5H19a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"
        />
      </svg>
    </button>
    <button :class="BUTTON_CLASS" type="button" title="Open terminal here">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
        <rect x="3" y="4" width="18" height="16" rx="2" />
        <path stroke-linecap="round" stroke-linejoin="round" d="m7 9 3 3-3 3M13 15h4" />
      </svg>
    </button>
  </div>
</template>
