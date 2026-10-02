<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import { useServicesStore } from '@/stores/services'
import { useProjectsStore } from '@/stores/projects'
import { useDatabasesStore } from '@/stores/databases'
import { usePhpStore } from '@/stores/php'
import { useLogsStore } from '@/stores/logs'
import { useChangelogStore } from '@/stores/changelog'
import { useUpdateStore } from '@/stores/update'
import { useUptime } from '@/composables/useUptime'

const route = useRoute()
const servicesStore = useServicesStore()
const projectsStore = useProjectsStore()
const databasesStore = useDatabasesStore()
const phpStore = usePhpStore()
const logsStore = useLogsStore()
const changelogStore = useChangelogStore()
const updateStore = useUpdateStore()
const { label: uptimeLabel } = useUptime()

// Fetched here (not just on the Changelog page itself) so the "new release"
// badge can show without the user having visited it yet.
onMounted(() => {
  changelogStore.fetchAll()
  updateStore.checkForUpdate()
})

const hasUnseenChangelog = computed(() => {
  const newest = changelogStore.entries[0]?.version
  return !!newest && newest !== changelogStore.lastSeenVersion
})

// A binary update and a "new" changelog entry are independent signals (one
// can exist without the other), but the nav row only has room for one dot —
// it lights up if either is true.
const hasChangelogAlert = computed(() => hasUnseenChangelog.value || updateStore.available !== null)

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
    to: '/php-extensions',
    icon: 'puzzle' as const,
    label: 'PHP Extensions',
    badge: '',
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
    to: '/support',
    icon: 'support' as const,
    // Labelled "Feedback", not "Support": v3 adds a "Support Developer"
    // (donate) menu, and two neighbouring entries reading "Support" would
    // send bug reports to the donation page. The route, store and API path
    // stay `support` - that is what the backend contract calls the endpoint.
    label: 'Feedback',
    badge: '',
    variant: 'default' as const,
  },
  {
    to: '/changelog',
    icon: 'changelog' as const,
    label: 'Changelog',
    badge: hasChangelogAlert.value ? '•' : '',
    variant: 'alert' as const,
  },
  {
    to: '/donate',
    icon: 'heart' as const,
    label: 'Support Developer',
    badge: '',
    variant: 'default' as const,
  },
  {
    to: '/appearance',
    icon: 'palette' as const,
    label: 'Appearance',
    badge: '',
    variant: 'default' as const,
  },
  {
    to: '/decorations',
    icon: 'sticker' as const,
    label: 'Decorations',
    badge: '',
    variant: 'default' as const,
  },
  // Last on purpose: the daily work (services, projects, databases) comes
  // first, and settings are visited rarely enough to sit out of the way.
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
  <aside class="glass flex w-60 shrink-0 flex-col gap-3 rounded-3xl p-3">
    <!-- Scrolls rather than clips when the window is at its minimum height;
         at the default size every item fits. -->
    <nav class="flex min-h-0 flex-col gap-0.5 overflow-y-auto [scrollbar-width:thin]">
      <!-- Inactive items are bare so the panel reads as one sheet of glass;
           the transparent border keeps rows from shifting when one becomes
           active and picks up a real edge. -->
      <RouterLink
        v-for="item in navItems"
        :key="item.to"
        :to="item.to"
        class="flex items-center gap-3 rounded-xl px-2.5 py-1.5 transition"
        :class="
          isActive(item.to)
            ? 'glass-selected'
            : 'border border-transparent hover:bg-white/50 dark:hover:bg-white/5'
        "
      >
        <span
          class="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg transition"
          :class="
            isActive(item.to)
              ? 'glass-accent'
              : 'glass-inset text-neutral-500 dark:text-neutral-400'
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
            v-else-if="item.icon === 'puzzle'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M9 4h3a1 1 0 0 1 1 1v1.5a1.5 1.5 0 0 0 3 0V5a1 1 0 0 1 1-1h1a2 2 0 0 1 2 2v1a1 1 0 0 1-1 1h-1.5a1.5 1.5 0 0 0 0 3H19a1 1 0 0 1 1 1v3a2 2 0 0 1-2 2h-1a1 1 0 0 1-1-1v-1.5a1.5 1.5 0 0 0-3 0V17a1 1 0 0 1-1 1H8a2 2 0 0 1-2-2v-1a1 1 0 0 1 1-1h1.5a1.5 1.5 0 0 0 0-3H7a1 1 0 0 1-1-1V7a2 2 0 0 1 2-2h1Z"
            />
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
            v-else-if="item.icon === 'support'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 20l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5Z"
            />
          </svg>
          <svg
            v-else-if="item.icon === 'changelog'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M12 8v4l3 3m6-3a9 9 0 1 1-9-9 9 9 0 0 1 9 9Z"
            />
          </svg>
          <svg
            v-else-if="item.icon === 'heart'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M12 21s-6.716-4.35-9.428-8.06C1.02 10.94 1.5 7.5 4.5 5.9c2.2-1.17 4.53-.4 5.9 1.4l1.6 2.1 1.6-2.1c1.37-1.8 3.7-2.57 5.9-1.4 3 1.6 3.48 5.04 1.93 7.04C18.716 16.65 12 21 12 21Z"
            />
          </svg>
          <svg
            v-else-if="item.icon === 'palette'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M12 21a9 9 0 1 1 9-9c0 1.66-1.34 3-3 3h-1.5a1.5 1.5 0 0 0-1.06 2.56A1.5 1.5 0 0 1 14.38 21H12Z"
            />
            <circle cx="7.5" cy="11.5" r="1" fill="currentColor" stroke="none" />
            <circle cx="10.5" cy="7.5" r="1" fill="currentColor" stroke="none" />
            <circle cx="15" cy="8.5" r="1" fill="currentColor" stroke="none" />
          </svg>
          <svg
            v-else-if="item.icon === 'sticker'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M12 3.5c.5 3.6 2 5.6 5.5 6.2-3.5.6-5 2.6-5.5 6.2-.5-3.6-2-5.6-5.5-6.2 3.5-.6 5-2.6 5.5-6.2Z"
            />
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M18.5 15.5c.2 1.4.8 2.1 2 2.3-1.2.2-1.8.9-2 2.3-.2-1.4-.8-2.1-2-2.3 1.2-.2 1.8-.9 2-2.3Z"
            />
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
              ? 'bg-accent-500/10 text-accent-600 dark:bg-accent-500/20 dark:text-accent-300'
              : 'text-neutral-500 dark:text-neutral-400'
          "
        >
          {{ item.badge }}
        </span>
      </RouterLink>
    </nav>

    <div class="glass-inset mt-auto shrink-0 rounded-2xl p-3">
      <div class="flex items-center gap-3">
        <div class="relative h-10 w-10 shrink-0">
          <svg viewBox="0 0 40 40" class="h-10 w-10 -rotate-90">
            <circle
              cx="20"
              cy="20"
              r="16"
              fill="none"
              stroke="currentColor"
              stroke-width="4"
              class="text-neutral-900/10 dark:text-white/10"
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

      <div class="glass-divider mt-2.5 flex items-center justify-between border-t pt-2.5 text-xs">
        <span class="text-neutral-500">Uptime</span>
        <span class="font-semibold">{{ uptimeLabel }}</span>
      </div>
    </div>
  </aside>
</template>
