<script setup lang="ts">
withDefaults(defineProps<{ variant?: 'neutral' | 'mono' }>(), { variant: 'neutral' })
</script>

<template>
  <span
    class="inline-flex items-center rounded-full bg-neutral-200/70 px-2 py-0.5 text-xs font-medium dark:bg-neutral-800"
    :class="
      variant === 'mono'
        ? 'font-mono text-neutral-500 dark:text-neutral-400'
        : 'text-neutral-600 dark:text-neutral-300'
    "
  >
    <slot />
  </span>
</template>
