<script setup lang="ts">
import { RouterLink, RouterView } from 'vue-router'
import { computed, onMounted } from 'vue'
import { useServicesStore } from '@/stores/services'
import { useProjectsStore } from '@/stores/projects'
import { useTheme } from '@/composables/useTheme'
import { useUptime } from '@/composables/useUptime'

const servicesStore = useServicesStore()
const projectsStore = useProjectsStore()
const { theme, toggle } = useTheme()
const { label: uptimeLabel } = useUptime()

onMounted(() => {
  servicesStore.fetchAll()
})

const navItems = computed(() => [
  {
    to: '/',
    label: 'Services',
    badge: `${servicesStore.runningCount}/${servicesStore.services.length}`,
  },
  { to: '/projects', label: 'Projects', badge: String(projectsStore.projects.length) },
])

const RING_RADIUS = 16
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS

const ringOffset = computed(() => {
  const total = servicesStore.services.length
  const ratio = total === 0 ? 0 : servicesStore.runningCount / total
  return RING_CIRCUMFERENCE * (1 - ratio)
})
</script>

<template>
  <div
    class="flex h-screen bg-neutral-50 text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100"
  >
    <aside
      class="flex w-56 shrink-0 flex-col border-r border-neutral-200 p-4 dark:border-neutral-800"
    >
      <div class="mb-6 flex items-center justify-between">
        <div class="flex items-center gap-2">
          <span class="h-2.5 w-2.5 rounded-full bg-red-600"></span>
          <span class="text-sm font-semibold tracking-wide">Rezure</span>
        </div>

        <button
          type="button"
          title="Toggle theme"
          class="flex h-7 w-7 items-center justify-center rounded-full border border-neutral-200 text-neutral-500 hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-400 dark:hover:bg-neutral-800"
          @click="toggle"
        >
          <svg v-if="theme === 'dark'" viewBox="0 0 24 24" fill="currentColor" class="h-3.5 w-3.5">
            <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79Z" />
          </svg>
          <svg
            v-else
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-3.5 w-3.5"
          >
            <circle cx="12" cy="12" r="4" />
            <path
              stroke-linecap="round"
              d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41"
            />
          </svg>
        </button>
      </div>

      <nav class="flex flex-col gap-1">
        <RouterLink
          v-for="item in navItems"
          :key="item.to"
          :to="item.to"
          class="flex items-center justify-between rounded-md px-3 py-2 text-sm font-medium text-neutral-600 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-900"
          active-class="!bg-red-600/10 !text-red-600 dark:!text-red-500"
        >
          <span>{{ item.label }}</span>
          <span
            class="rounded-full bg-neutral-100 px-1.5 py-0.5 text-xs text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400"
          >
            {{ item.badge }}
          </span>
        </RouterLink>
      </nav>

      <div class="mt-auto rounded-xl border border-neutral-200 p-4 dark:border-neutral-800">
        <div class="flex items-center gap-3">
          <svg viewBox="0 0 40 40" class="h-10 w-10 -rotate-90">
            <circle
              cx="20"
              cy="20"
              r="16"
              fill="none"
              stroke="currentColor"
              stroke-width="4"
              class="text-neutral-200 dark:text-neutral-800"
            />
            <circle
              cx="20"
              cy="20"
              r="16"
              fill="none"
              stroke="currentColor"
              stroke-width="4"
              stroke-linecap="round"
              class="text-emerald-500"
              :stroke-dasharray="RING_CIRCUMFERENCE"
              :stroke-dashoffset="ringOffset"
            />
          </svg>
          <div>
            <p class="text-sm font-semibold">{{ servicesStore.runningCount }} services up</p>
            <p class="text-xs text-neutral-500">of {{ servicesStore.services.length }} installed</p>
          </div>
        </div>
        <div
          class="mt-3 flex items-center justify-between border-t border-neutral-200 pt-3 text-xs dark:border-neutral-800"
        >
          <span class="text-neutral-500">Uptime</span>
          <span class="font-medium">{{ uptimeLabel }}</span>
        </div>
      </div>
    </aside>

    <main class="min-w-0 flex-1 overflow-y-auto p-6">
      <RouterView />
    </main>
  </div>
</template>
