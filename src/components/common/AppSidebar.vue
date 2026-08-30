<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import { useServicesStore } from '@/stores/services'
import { useProjectsStore } from '@/stores/projects'
import { useDatabasesStore } from '@/stores/databases'
import { usePhpStore } from '@/stores/php'
import { useLogsStore } from '@/stores/logs'
import { useUptime } from '@/composables/useUptime'

const route = useRoute()
const servicesStore = useServicesStore()
const projectsStore = useProjectsStore()
const databasesStore = useDatabasesStore()
const phpStore = usePhpStore()
const logsStore = useLogsStore()
const { label: uptimeLabel } = useUptime()

const navItems = computed(() => [
  {
    to: '/',
    icon: 'pulse' as const,
    label: 'Services',
    badge: `${servicesStore.runningCount}/${servicesStore.services.length}`,
    variant: 'default' as const,
  },
  {
    to: '/projects',
    icon: 'folder' as const,
    label: 'Projects',
    badge: String(projectsStore.projects.length),
    variant: 'default' as const,
  },
  {
    to: '/databases',
    icon: 'database' as const,
    label: 'Databases',
    badge: String(databasesStore.databases.length),
    variant: 'default' as const,
  },
  {
    to: '/switch',
    icon: 'switch' as const,
    label: 'Switch',
    badge: String(phpStore.versions.length),
    variant: 'default' as const,
  },
  {
    to: '/logs',
    icon: 'logs' as const,
    label: 'Logs',
    badge: logsStore.errorCount > 0 ? String(logsStore.errorCount) : '',
    variant: 'alert' as const,
  },
  {
    to: '/settings',
    icon: 'settings' as const,
    label: 'Settings',
    badge: '',
    variant: 'default' as const,
  },
])

// Matched explicitly rather than via `router-link-active`: the root path is a
// prefix of every route, so the default (non-exact) active class would light up
// every item at once.
const isActive = (to: string) => route.path === to

const RING_RADIUS = 16
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS

const ringOffset = computed(() => {
  const total = servicesStore.services.length
  const ratio = total === 0 ? 0 : servicesStore.runningCount / total
  return RING_CIRCUMFERENCE * (1 - ratio)
})
</script>

<template>
  <aside
    class="flex w-64 shrink-0 flex-col gap-3 border-r border-neutral-200/70 p-4 dark:border-neutral-800"
  >
    <nav class="flex flex-col gap-2">
      <RouterLink
        v-for="item in navItems"
        :key="item.to"
        :to="item.to"
        class="flex items-center gap-3 rounded-2xl border px-3 py-2.5 transition"
        :class="
          isActive(item.to)
            ? 'border-red-200 bg-red-50 dark:border-red-500/30 dark:bg-red-500/10'
            : 'border-neutral-200 bg-white/70 hover:border-neutral-300 dark:border-neutral-800 dark:bg-neutral-900/60 dark:hover:border-neutral-700'
        "
      >
        <span
          class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl transition"
          :class="
            isActive(item.to)
              ? 'bg-red-600 text-white shadow-sm shadow-red-600/30'
              : 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
          "
        >
          <svg
            v-if="item.icon === 'pulse'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path stroke-linecap="round" stroke-linejoin="round" d="M3 12h4l3 8 4-16 3 8h4" />
          </svg>
          <svg
            v-else-if="item.icon === 'folder'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M3 7a2 2 0 0 1 2-2h3.6l2 2.5H19a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"
            />
          </svg>
          <svg
            v-else-if="item.icon === 'database'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <ellipse cx="12" cy="6" rx="7" ry="3" />
            <path
              stroke-linecap="round"
              d="M5 6v12c0 1.7 3.1 3 7 3s7-1.3 7-3V6M5 12c0 1.7 3.1 3 7 3s7-1.3 7-3"
            />
          </svg>
          <svg
            v-else-if="item.icon === 'switch'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path stroke-linecap="round" stroke-linejoin="round" d="M4 8h13l-3-3M20 16H7l3 3" />
          </svg>
          <svg
            v-else-if="item.icon === 'logs'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path stroke-linecap="round" stroke-linejoin="round" d="M4 6h16M4 12h16M4 18h10" />
          </svg>
          <svg
            v-else
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 0 0 2.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 0 0 1.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 0 0-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 0 0-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 0 0-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 0 0-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 0 0 1.066-2.573c-.94-1.543.826-3.31 2.37-2.37a1.724 1.724 0 0 0 2.572-1.065Z"
            />
            <circle cx="12" cy="12" r="3" />
          </svg>
        </span>

        <span
          class="flex-1 text-sm"
          :class="
            isActive(item.to)
              ? 'font-semibold text-neutral-900 dark:text-neutral-100'
              : 'font-medium text-neutral-600 dark:text-neutral-400'
          "
        >
          {{ item.label }}
        </span>

        <span
          v-if="item.variant === 'alert' && item.badge"
          class="flex h-5 min-w-5 items-center justify-center rounded-full bg-red-500 px-1.5 text-xs font-semibold text-white"
        >
          {{ item.badge }}
        </span>
        <span
          v-else-if="item.badge"
          class="rounded-full px-2 py-0.5 text-xs font-medium"
          :class="
            isActive(item.to)
              ? 'bg-white/80 text-red-600 dark:bg-neutral-900/60 dark:text-red-400'
              : 'text-neutral-500 dark:text-neutral-400'
          "
        >
          {{ item.badge }}
        </span>
      </RouterLink>
    </nav>

    <div
      class="mt-auto rounded-2xl border border-neutral-200 bg-white/70 p-4 dark:border-neutral-800 dark:bg-neutral-900/60"
    >
      <div class="flex items-center gap-3">
        <div class="relative h-11 w-11 shrink-0">
          <svg viewBox="0 0 40 40" class="h-11 w-11 -rotate-90">
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
              class="text-emerald-500 transition-all duration-500"
              :stroke-dasharray="RING_CIRCUMFERENCE"
              :stroke-dashoffset="ringOffset"
            />
          </svg>
          <span class="absolute inset-0 flex items-center justify-center text-sm font-semibold">
            {{ servicesStore.runningCount }}
          </span>
        </div>
        <div>
          <p class="text-sm font-semibold">Services up</p>
          <p class="text-xs text-neutral-500">of {{ servicesStore.services.length }} installed</p>
        </div>
      </div>

      <div
        class="mt-3 flex items-center justify-between border-t border-neutral-200 pt-3 text-xs dark:border-neutral-800"
      >
        <span class="text-neutral-500">Uptime</span>
        <span class="font-semibold">{{ uptimeLabel }}</span>
      </div>
    </div>
  </aside>
</template>
