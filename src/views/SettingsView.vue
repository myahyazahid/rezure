<script setup lang="ts">
import { onActivated } from 'vue'
import { useSettingsStore } from '@/stores/settings'

const settingsStore = useSettingsStore()

// `onActivated`, not `onMounted`: the view is kept alive, so it mounts once
// and this is what runs on every return to the page. It fires on the first
// mount too, so nothing is lost by the swap.
onActivated(() => {
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

const DOMAIN_SUFFIXES = ['test', 'local', 'dev'] as const
</script>

<template>
  <section>
    <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">Settings</h1>
    <p class="mt-1 text-sm text-neutral-500">Configure paths, ports, and PHP versions.</p>

    <p v-if="settingsStore.error" class="mt-3 text-sm text-red-600 dark:text-red-400">
      {{ settingsStore.error }}
    </p>

    <div class="mt-6">
      <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">General</h2>
      <p class="mt-0.5 text-xs text-neutral-500">
        How Rezure starts and what it does in the background.
      </p>

      <div
        class="mt-3 divide-y divide-neutral-200/80 rounded-2xl border border-neutral-200 bg-white dark:divide-neutral-800 dark:border-neutral-800 dark:bg-neutral-900/60"
      >
        <div class="flex items-center justify-between gap-4 p-4">
          <div>
            <p class="font-semibold text-neutral-900 dark:text-neutral-100">
              Start Rezure with Windows
            </p>
            <p class="mt-0.5 text-xs text-neutral-500">
              Services boot in the background at sign-in.
            </p>
          </div>
          <button
            type="button"
            role="switch"
            :aria-checked="settingsStore.startWithWindows"
            class="relative h-6 w-11 shrink-0 rounded-full transition"
            :class="
              settingsStore.startWithWindows ? 'bg-red-600' : 'bg-neutral-200 dark:bg-neutral-700'
            "
            @click="settingsStore.setStartWithWindows(!settingsStore.startWithWindows)"
          >
            <span
              class="absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition"
              :class="settingsStore.startWithWindows ? 'left-5' : 'left-0.5'"
            />
          </button>
        </div>

        <div class="flex items-center justify-between gap-4 p-4">
          <div>
            <p class="font-semibold text-neutral-900 dark:text-neutral-100">
              Keep running in tray on close
            </p>
            <p class="mt-0.5 text-xs text-neutral-500">Closing the window leaves services up.</p>
          </div>
          <button
            type="button"
            role="switch"
            :aria-checked="settingsStore.keepInTrayOnClose"
            class="relative h-6 w-11 shrink-0 rounded-full transition"
            :class="
              settingsStore.keepInTrayOnClose ? 'bg-red-600' : 'bg-neutral-200 dark:bg-neutral-700'
            "
            @click="settingsStore.setKeepInTrayOnClose(!settingsStore.keepInTrayOnClose)"
          >
            <span
              class="absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition"
              :class="settingsStore.keepInTrayOnClose ? 'left-5' : 'left-0.5'"
            />
          </button>
        </div>

        <div class="flex items-center justify-between gap-4 p-4">
          <div>
            <p class="font-semibold text-neutral-900 dark:text-neutral-100">
              Notify when a service crashes
            </p>
            <p class="mt-0.5 text-xs text-neutral-500">
              A tray toast the moment a process exits unexpectedly.
            </p>
          </div>
          <button
            type="button"
            role="switch"
            :aria-checked="settingsStore.notifyOnCrash"
            class="relative h-6 w-11 shrink-0 rounded-full transition"
            :class="
              settingsStore.notifyOnCrash ? 'bg-red-600' : 'bg-neutral-200 dark:bg-neutral-700'
            "
            @click="settingsStore.setNotifyOnCrash(!settingsStore.notifyOnCrash)"
          >
            <span
              class="absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition"
              :class="settingsStore.notifyOnCrash ? 'left-5' : 'left-0.5'"
            />
          </button>
        </div>
      </div>
    </div>

    <div class="mt-6">
      <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">Domains</h2>
      <p class="mt-0.5 text-xs text-neutral-500">Applied to every generated virtual host.</p>

      <div
        class="mt-3 divide-y divide-neutral-200/80 rounded-2xl border border-neutral-200 bg-white dark:divide-neutral-800 dark:border-neutral-800 dark:bg-neutral-900/60"
      >
        <div class="flex items-center justify-between gap-4 p-4">
          <div>
            <p class="font-semibold text-neutral-900 dark:text-neutral-100">Domain suffix</p>
            <p class="mt-0.5 text-xs text-neutral-500">
              Only applies to new or resynced projects — existing ones keep their current domain.
            </p>
          </div>
          <div
            class="flex shrink-0 gap-1 rounded-full border border-neutral-200 p-0.5 dark:border-neutral-700"
          >
            <button
              v-for="suffix in DOMAIN_SUFFIXES"
              :key="suffix"
              type="button"
              class="rounded-full px-3 py-1 text-xs font-medium transition"
              :class="
                settingsStore.domainSuffix === suffix
                  ? 'bg-red-600 text-white'
                  : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-100'
              "
              @click="settingsStore.setDomainSuffix(suffix)"
            >
              .{{ suffix }}
            </button>
          </div>
        </div>

        <div class="flex items-center justify-between gap-4 p-4">
          <div>
            <p class="font-semibold text-neutral-900 dark:text-neutral-100">
              Auto-write hosts entries
            </p>
            <p class="mt-0.5 text-xs text-neutral-500">
              Syncs once at startup so new projects resolve without an extra UAC prompt each time.
            </p>
          </div>
          <button
            type="button"
            role="switch"
            :aria-checked="settingsStore.autoWriteHosts"
            class="relative h-6 w-11 shrink-0 rounded-full transition"
            :class="
              settingsStore.autoWriteHosts ? 'bg-red-600' : 'bg-neutral-200 dark:bg-neutral-700'
            "
            @click="settingsStore.setAutoWriteHosts(!settingsStore.autoWriteHosts)"
          >
            <span
              class="absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition"
              :class="settingsStore.autoWriteHosts ? 'left-5' : 'left-0.5'"
            />
          </button>
        </div>
      </div>
    </div>

    <div
      class="mt-6 rounded-2xl border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900/60"
    >
      <div
        class="flex items-center justify-between gap-4 border-b border-neutral-200/80 p-4 dark:border-neutral-800"
      >
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
            Anonymous and opt-in: a random device id, the app and OS version, and which services you
            start or stop. Never your project names, paths, or file contents. Off means nothing is
            recorded at all, not merely that nothing is sent.
          </p>
        </div>
        <button
          type="button"
          role="switch"
          :aria-checked="settingsStore.shareUsageData"
          class="relative h-6 w-11 shrink-0 rounded-full transition"
          :class="
            settingsStore.shareUsageData ? 'bg-red-600' : 'bg-neutral-200 dark:bg-neutral-700'
          "
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
      <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
        Where things live
      </h2>
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
