<script setup lang="ts">
withDefaults(defineProps<{ variant?: 'neutral' | 'mono' }>(), { variant: 'neutral' })
</script>

<template>
  <span
    class="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium"
    :class="
      variant === 'mono'
        ? 'bg-neutral-100 font-mono text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
        : 'bg-neutral-100 text-neutral-600 dark:bg-neutral-800 dark:text-neutral-300'
    "
  >
    <slot />
  </span>
</template>
