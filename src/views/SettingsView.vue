<script setup lang="ts">
import { onMounted } from 'vue'
import { useSettingsStore } from '@/stores/settings'

const settingsStore = useSettingsStore()

onMounted(() => {
  settingsStore.fetchAll()
  settingsStore.fetchStoragePaths()
})

function onPortChange(event: Event) {
  const port = Number((event.target as HTMLInputElement).value)
  if (Number.isFinite(port) && port > 0 && port <= 65535) {
    settingsStore.setDefaultPort(port)
  }
}

const PATH_ROWS = [
  { key: 'wwwRoot', label: 'Projects' },
  { key: 'binariesDir', label: 'Downloaded binaries' },
  { key: 'dropInDir', label: 'PHP drop-in folder' },
  { key: 'dumpsDir', label: 'Database exports' },
] as const
</script>

<template>
  <section class="max-w-2xl">
    <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">Settings</h1>
    <p class="mt-1 text-sm text-neutral-500">Configure paths, ports, and PHP versions.</p>

    <p v-if="settingsStore.error" class="mt-3 text-sm text-red-600 dark:text-red-400">
      {{ settingsStore.error }}
    </p>

    <div
      class="mt-6 rounded-2xl border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900/60"
    >
      <div class="flex items-center justify-between gap-4 border-b border-neutral-200/80 p-4 dark:border-neutral-800">
        <div>
          <p class="font-semibold text-neutral-900 dark:text-neutral-100">Default port</p>
          <p class="mt-0.5 text-xs text-neutral-500">Used to pre-fill new virtual hosts.</p>
        </div>
        <input
          type="number"
          min="1"
          max="65535"
          :value="settingsStore.defaultPort"
          class="w-24 rounded-lg border border-neutral-200 bg-white px-3 py-1.5 text-right text-sm text-neutral-900 focus:border-red-500 focus:outline-none dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
          @change="onPortChange"
        />
      </div>

      <div class="flex items-center justify-between gap-4 p-4">
        <div>
          <p class="font-semibold text-neutral-900 dark:text-neutral-100">Share usage data</p>
          <p class="mt-0.5 text-xs text-neutral-500">
            Anonymous, opt-in — not collected yet (planned for v2's telemetry).
          </p>
        </div>
        <button
          type="button"
          role="switch"
          :aria-checked="settingsStore.shareUsageData"
          class="relative h-6 w-11 shrink-0 rounded-full transition"
          :class="settingsStore.shareUsageData ? 'bg-red-600' : 'bg-neutral-200 dark:bg-neutral-700'"
          @click="settingsStore.setShareUsageData(!settingsStore.shareUsageData)"
        >
          <span
            class="absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition"
            :class="settingsStore.shareUsageData ? 'left-5' : 'left-0.5'"
          />
        </button>
      </div>
    </div>

    <div class="mt-6">
      <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">Where things live</h2>
      <p class="mt-0.5 text-xs text-neutral-500">
        Read-only — Rezure scans these folders directly, so there's nothing to redirect.
      </p>

      <div
        class="mt-3 divide-y divide-neutral-200/80 rounded-2xl border border-neutral-200 bg-white dark:divide-neutral-800 dark:border-neutral-800 dark:bg-neutral-900/60"
      >
        <div
          v-for="row in PATH_ROWS"
          :key="row.key"
          class="flex items-center justify-between gap-4 p-4 text-sm"
        >
          <span class="shrink-0 text-neutral-500">{{ row.label }}</span>
          <span
            v-if="settingsStore.storagePaths"
            class="truncate font-mono text-xs text-neutral-700 dark:text-neutral-300"
            :title="settingsStore.storagePaths[row.key]"
          >
            {{ settingsStore.storagePaths[row.key] }}
          </span>
          <span v-else class="text-xs text-neutral-400">—</span>
        </div>
      </div>
    </div>
  </section>
</template>
