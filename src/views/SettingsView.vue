<script setup lang="ts">
import { useSettingsStore } from '@/stores/settings'

const settingsStore = useSettingsStore()
</script>

<template>
  <section>
    <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">Settings</h1>
    <p class="mt-1 text-sm text-neutral-500">Configure paths, ports, and PHP versions.</p>

    <div class="mt-6 text-sm text-neutral-500">Default port: {{ settingsStore.defaultPort }}</div>
  </section>
</template>
