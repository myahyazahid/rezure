<script setup lang="ts">
import { RouterLink, RouterView } from 'vue-router'

const navItems = [
  { to: '/', label: 'Dashboard' },
  { to: '/projects', label: 'Projects' },
  { to: '/settings', label: 'Settings' },
]
</script>

<template>
  <div
    class="flex h-screen bg-neutral-50 text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100"
  >
    <aside class="w-52 shrink-0 border-r border-neutral-200 p-4 dark:border-neutral-800">
      <div class="mb-6 flex items-center gap-2">
        <span class="h-2.5 w-2.5 rounded-full bg-red-600"></span>
        <span class="text-sm font-semibold tracking-wide">Rezure</span>
      </div>

      <nav class="flex flex-col gap-1">
        <RouterLink
          v-for="item in navItems"
          :key="item.to"
          :to="item.to"
          class="rounded-md px-3 py-2 text-sm font-medium text-neutral-600 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-900"
          active-class="!bg-red-600/10 !text-red-600 dark:!text-red-500"
        >
          {{ item.label }}
        </RouterLink>
      </nav>
    </aside>

    <main class="flex-1 overflow-y-auto p-6">
      <RouterView />
    </main>
  </div>
</template>
