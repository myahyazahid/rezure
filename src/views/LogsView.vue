<script setup lang="ts">
import { computed, ref } from 'vue'
import { LOG_SERVICES, useLogsStore } from '@/stores/logs'
import { useServicesStore } from '@/stores/services'
import type { LogLevel } from '@/types/log'
import SearchInput from '@/components/common/SearchInput.vue'

const store = useLogsStore()
const servicesStore = useServicesStore()

const search = ref('')
const selectedService = ref<string | null>(null)
const selectedLevel = ref<LogLevel | null>(null)

/** Pooled PHP instances (`php-8.3.0`, ...) aren't in the static
 *  `LOG_SERVICES` list — they're registered dynamically per pinned
 *  version (see `services::php_pool`). Add one filter shortcut per
 *  instance currently known to the services store, alongside the fixed
 *  ones, so their logs get the same one-click filter. */
const pooledPhpServices = computed(() =>
  servicesStore.services
    .map((s) => s.id)
    .filter((id) => id.startsWith('php-'))
    .sort(),
)

const filterableServices = computed(() => [...LOG_SERVICES, ...pooledPhpServices.value])

const LEVELS: { value: LogLevel | null; label: string }[] = [
  { value: null, label: 'All' },
  { value: 'info', label: 'Info' },
  { value: 'warn', label: 'Warn' },
  { value: 'error', label: 'Error' },
]

/** The chosen segment of the service and level pickers. */
const SEGMENT_ON_CLASS = 'glass-raised text-neutral-900 dark:text-neutral-50'

const filtered = computed(() =>
  store.filtered(selectedService.value, selectedLevel.value, search.value),
)

function levelClass(level: LogLevel) {
  if (level === 'error') return 'font-semibold text-red-600 dark:text-red-400'
  if (level === 'warn') return 'font-semibold text-amber-600 dark:text-amber-400'
  return 'text-neutral-500'
}
</script>

<template>
  <section>
    <div class="flex items-start justify-between gap-4">
      <div>
        <h1 class="text-[28px] leading-tight font-bold tracking-tight">Logs</h1>
        <p class="mt-1 text-sm text-neutral-500">Combined tail across every service.</p>
      </div>

      <div class="flex shrink-0 items-center gap-2">
        <!-- Same language as a service's Start/Stop: one neutral button whose
             icon carries the state — green play to resume, accent-coloured bars to pause. -->
        <button
          type="button"
          class="glass-accent flex items-center gap-2 rounded-full px-4 py-2 text-sm font-semibold transition"
          @click="store.togglePause"
        >
          <svg
            v-if="store.paused"
            viewBox="0 0 10 10"
            fill="currentColor"
            aria-hidden="true"
            class="h-2.5 w-2.5 text-emerald-500"
          >
            <path d="M1.5 0.8 9 5 1.5 9.2Z" />
          </svg>
          <svg
            v-else
            viewBox="0 0 10 10"
            fill="currentColor"
            aria-hidden="true"
            class="h-2.5 w-2.5"
          >
            <rect x="1.5" y="1" width="2.5" height="8" rx="0.8" />
            <rect x="6" y="1" width="2.5" height="8" rx="0.8" />
          </svg>
          {{ store.paused ? 'Resume' : 'Pause' }}
        </button>
        <button
          type="button"
          class="glass-btn rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition dark:text-neutral-200"
          @click="store.clear"
        >
          Clear
        </button>
      </div>
    </div>

    <div class="mt-5 flex flex-wrap items-center gap-2">
      <SearchInput v-model="search" placeholder="Filter log text" class="min-w-55 flex-1" />

      <!-- A segmented control, like the level picker beside it: the chosen
           filter is the lifted segment, the rest stay bare until hovered. -->
      <div class="glass flex flex-wrap items-center gap-0.5 rounded-2xl p-1">
        <button
          type="button"
          class="rounded-full px-3 py-1 text-sm font-medium transition"
          :class="selectedService === null ? SEGMENT_ON_CLASS : 'glass-ghost'"
          @click="selectedService = null"
        >
          All services
        </button>
        <button
          v-for="service in filterableServices"
          :key="service"
          type="button"
          class="rounded-full px-3 py-1 text-sm font-medium transition"
          :class="selectedService === service ? SEGMENT_ON_CLASS : 'glass-ghost'"
          @click="selectedService = service"
        >
          {{ service }}
        </button>
      </div>

      <div class="glass ml-auto flex shrink-0 items-center gap-0.5 rounded-full p-1">
        <button
          v-for="level in LEVELS"
          :key="level.label"
          type="button"
          class="rounded-full px-3 py-1 text-xs font-semibold transition"
          :class="selectedLevel === level.value ? SEGMENT_ON_CLASS : 'glass-ghost'"
          @click="selectedLevel = level.value"
        >
          {{ level.label }}
        </button>
      </div>
    </div>

    <div class="glass mt-4 rounded-2xl">
      <div v-if="filtered.length === 0" class="p-6 text-center text-sm text-neutral-500">
        No log lines match the current filters.
      </div>
      <div v-else class="max-h-[calc(100vh-360px)] overflow-y-auto p-2">
        <div
          v-for="entry in filtered"
          :key="entry.id"
          class="flex items-start gap-3 rounded-lg px-2.5 py-1.5 font-mono text-xs hover:bg-white/50 dark:hover:bg-white/5"
        >
          <span class="w-16 shrink-0 text-neutral-400">{{ entry.time }}</span>
          <span class="w-16 shrink-0 text-neutral-500">{{ entry.service }}</span>
          <span class="w-14 shrink-0" :class="levelClass(entry.level)">{{
            entry.level.toUpperCase()
          }}</span>
          <span class="min-w-0 flex-1 text-neutral-700 dark:text-neutral-300">{{
            entry.message
          }}</span>
        </div>
      </div>

      <div
        class="glass-divider flex items-center justify-between border-t px-4 py-2.5 text-xs text-neutral-500"
      >
        <span>{{ filtered.length }} lines</span>
        <span>{{ store.paused ? 'Paused' : 'Live tail' }}</span>
      </div>
    </div>
  </section>
</template>
