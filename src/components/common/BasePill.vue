<script setup lang="ts">
withDefaults(defineProps<{ variant?: 'neutral' | 'mono' }>(), { variant: 'neutral' })
</script>

<template>
  <span
    class="glass-inset inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium"
    :class="
      variant === 'mono'
        ? 'font-mono text-neutral-500 dark:text-neutral-400'
        : 'text-neutral-600 dark:text-neutral-300'
    "
  >
    <slot />
  </span>
</template>
