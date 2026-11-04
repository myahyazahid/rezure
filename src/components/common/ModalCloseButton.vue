<script setup lang="ts">
/**
 * The X in a modal's top-right corner. Modals don't close on a click outside
 * them — a stray click shouldn't throw away what was typed — so this, a
 * Cancel/Close button, or Escape are the ways out. Sits absolutely: the
 * modal's panel needs `relative`, and its header room on the right (`pr-8`).
 * `@click` falls through to the button; the parent decides what closing does.
 */
defineProps<{
  /** While the modal is mid-action and closing would abandon it. */
  disabled?: boolean
}>()
</script>

<template>
  <button
    type="button"
    aria-label="Close"
    title="Close"
    class="glass-btn absolute top-4 right-4 z-10 flex h-8 w-8 items-center justify-center rounded-full text-neutral-500 transition disabled:opacity-40 dark:text-neutral-300"
    :disabled="disabled"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="h-4 w-4">
      <path stroke-linecap="round" d="M6 6l12 12M18 6L6 18" />
    </svg>
  </button>
</template>
