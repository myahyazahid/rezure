<script setup lang="ts">
import { computed } from 'vue'
import type { InstallProgress } from '@/types/binary'

export interface CatalogEntry {
  version: string
  latest: boolean
  installed: boolean
  /** YYYY-MM-DD, when the runtime's index reports one (PHP, MariaDB, Node). */
  released?: string
  /** Archive size as the index reports it (PHP only). */
  size?: string
  /** LTS codename, or null for a Current release (Node only). */
  lts?: string | null
}

const props = defineProps<{
  releases: CatalogEntry[]
  loading: boolean
  error: string | null
  /** The version currently downloading, or null. */
  installingVersion: string | null
  progress: (version: string) => InstallProgress | null
  /** Shown while loading and in the retry message, e.g. "php.net's release
   *  index". */
  sourceLabel: string
}>()

const emit = defineEmits<{ install: [version: string]; retry: [] }>()

const busy = computed(() => props.installingVersion !== null)

function stageLabel(version: string) {
  switch (props.progress(version)?.stage) {
    case 'downloading':
      return 'Downloading…'
    case 'verifying':
      return 'Verifying…'
    case 'extracting':
      return 'Extracting…'
    default:
      return 'Installing…'
  }
}

function progressPercent(version: string) {
  const p = props.progress(version)
  if (!p || !p.totalBytes) return null
  return Math.min(100, Math.round((p.downloadedBytes / p.totalBytes) * 100))
}
</script>

<template>
  <p v-if="loading" class="py-6 text-center text-sm text-neutral-500">Reading {{ sourceLabel }}…</p>

  <div
    v-else-if="error"
    class="rounded-2xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-900 dark:border-amber-500/25 dark:bg-amber-500/10 dark:text-amber-200"
  >
    Couldn't reach {{ sourceLabel }}: {{ error }}
    <button type="button" class="ml-1 font-semibold underline" @click="emit('retry')">Retry</button>
  </div>

  <p v-else-if="releases.length === 0" class="py-6 text-center text-sm text-neutral-500">
    Nothing to install from here right now.
  </p>

  <div v-else class="flex flex-col gap-2">
    <div
      v-for="release in releases"
      :key="release.version"
      class="rounded-2xl border border-neutral-200 bg-neutral-50/70 p-3.5 dark:border-neutral-800 dark:bg-neutral-900/60"
    >
      <div class="flex items-center gap-3">
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class="font-mono font-semibold text-neutral-900 dark:text-neutral-100">
              {{ release.version }}
            </span>
            <span
              v-if="release.latest"
              class="rounded-full bg-red-100 px-2 py-0.5 text-[10px] font-bold tracking-wide text-red-600 uppercase dark:bg-red-500/15 dark:text-red-400"
            >
              Latest
            </span>
            <span
              v-else-if="release.lts"
              class="rounded-full bg-emerald-100 px-2 py-0.5 text-[10px] font-bold tracking-wide text-emerald-600 uppercase dark:bg-emerald-500/15 dark:text-emerald-400"
            >
              LTS {{ release.lts }}
            </span>
          </div>
          <p class="mt-0.5 text-xs text-neutral-500">
            <template v-if="installingVersion === release.version">
              {{ stageLabel(release.version) }}
            </template>
            <template v-else-if="release.released || release.size">
              <template v-if="release.released">Released {{ release.released }}</template>
              <template v-if="release.released && release.size"> · </template>
              <template v-if="release.size">{{ release.size }}</template>
            </template>
          </p>
        </div>

        <button
          v-if="!release.installed"
          type="button"
          class="shrink-0 rounded-full bg-red-600 px-4 py-2 text-sm font-semibold text-white shadow-sm shadow-red-600/30 transition hover:bg-red-500 disabled:opacity-50"
          :disabled="busy"
          @click="emit('install', release.version)"
        >
          {{ installingVersion === release.version ? 'Installing…' : 'Install' }}
        </button>
        <span
          v-else
          class="shrink-0 rounded-full border border-neutral-200 px-4 py-2 text-sm font-semibold text-neutral-400 dark:border-neutral-700 dark:text-neutral-500"
        >
          Installed
        </span>
      </div>

      <div
        v-if="installingVersion === release.version"
        class="mt-3 h-1.5 overflow-hidden rounded-full bg-neutral-200/70 dark:bg-neutral-800"
      >
        <div
          class="h-full rounded-full bg-red-500 transition-all"
          :class="progressPercent(release.version) === null ? 'w-1/3 animate-pulse' : ''"
          :style="
            progressPercent(release.version) !== null
              ? { width: `${progressPercent(release.version)}%` }
              : undefined
          "
        ></div>
      </div>
    </div>
  </div>
</template>
