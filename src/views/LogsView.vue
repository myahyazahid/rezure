<script setup lang="ts">
import { computed, ref } from 'vue'
import { LOG_SERVICES, useLogsStore } from '@/stores/logs'
import type { LogLevel } from '@/types/log'
import SearchInput from '@/components/common/SearchInput.vue'

const store = useLogsStore()

const search = ref('')
const selectedService = ref<string | null>(null)
const selectedLevel = ref<LogLevel | null>(null)

const LEVELS: { value: LogLevel | null; label: string }[] = [
  { value: null, label: 'All' },
  { value: 'info', label: 'Info' },
  { value: 'warn', label: 'Warn' },
  { value: 'error', label: 'Error' },
]

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
        <button
          type="button"
          class="rounded-full px-4 py-2 text-sm font-semibold transition"
          :class="
            store.paused
              ? 'bg-neutral-200/70 text-neutral-700 hover:bg-neutral-300/70 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700'
              : 'bg-red-100 text-red-600 hover:bg-red-200 dark:bg-red-500/15 dark:text-red-400 dark:hover:bg-red-500/25'
          "
          @click="store.togglePause"
        >
          {{ store.paused ? 'Resume' : 'Pause' }}
        </button>
        <button
          type="button"
          class="rounded-full border border-neutral-200 bg-white/70 px-4 py-2 text-sm font-semibold text-neutral-700 transition hover:bg-white dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
          @click="store.clear"
        >
          Clear
        </button>
      </div>
    </div>

    <div class="mt-5 flex flex-wrap items-center gap-2">
      <SearchInput v-model="search" placeholder="Filter log text" class="min-w-55 flex-1" />

      <div class="flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          class="rounded-full px-3.5 py-1.5 text-sm font-medium transition"
          :class="
            selectedService === null
              ? 'bg-red-600 text-white shadow-sm shadow-red-600/30'
              : 'border border-neutral-200 bg-white/70 text-neutral-600 hover:bg-white dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-300 dark:hover:bg-neutral-800'
          "
          @click="selectedService = null"
        >
          All services
        </button>
        <button
          v-for="service in LOG_SERVICES"
          :key="service"
          type="button"
          class="rounded-full px-3.5 py-1.5 text-sm font-medium transition"
          :class="
            selectedService === service
              ? 'bg-red-600 text-white shadow-sm shadow-red-600/30'
              : 'border border-neutral-200 bg-white/70 text-neutral-600 hover:bg-white dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-300 dark:hover:bg-neutral-800'
          "
          @click="selectedService = service"
        >
          {{ service }}
        </button>
      </div>

      <div
        class="ml-auto flex shrink-0 items-center gap-0.5 rounded-full border border-neutral-200 bg-white/70 p-1 dark:border-neutral-700 dark:bg-neutral-900/60"
      >
        <button
          v-for="level in LEVELS"
          :key="level.label"
          type="button"
          class="rounded-full px-3 py-1 text-xs font-semibold transition"
          :class="
            selectedLevel === level.value
              ? 'bg-red-600 text-white'
              : 'text-neutral-500 hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-100'
          "
          @click="selectedLevel = level.value"
        >
          {{ level.label }}
        </button>
      </div>
    </div>

    <div
      class="mt-4 rounded-2xl border border-neutral-200/80 bg-neutral-100/60 dark:border-neutral-800 dark:bg-neutral-900/60"
    >
      <div v-if="filtered.length === 0" class="p-6 text-center text-sm text-neutral-500">
        No log lines match the current filters.
      </div>
      <div v-else class="max-h-[calc(100vh-360px)] overflow-y-auto p-2">
        <div
          v-for="entry in filtered"
          :key="entry.id"
          class="flex items-start gap-3 rounded-lg px-2.5 py-1.5 font-mono text-xs hover:bg-white/60 dark:hover:bg-neutral-800/60"
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
        class="flex items-center justify-between border-t border-neutral-200 px-4 py-2.5 text-xs text-neutral-500 dark:border-neutral-800"
      >
        <span>{{ filtered.length }} lines</span>
        <span>{{ store.paused ? 'Paused' : 'Live tail' }}</span>
      </div>
    </div>
  </section>
</template>
